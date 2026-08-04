//! Fuma - Fast Course Page Generator for HITSZ-OpenAuto
//!
//! This binary replaces the Python-based page generation system with a high-performance
//! Rust implementation that avoids the N+1 query problem by loading all data upfront.

mod constants;
mod error;
mod fetcher;
mod formatter;
mod generator;
mod linter;
mod loader;
mod models;
mod tree;

use error::Result;
use std::path::Path;
use std::{env, fs};

const COURSE_ORG: &str = "HOAHRB-Courses";

/// Main entry point for the Fuma course page generator.
///
/// This program:
/// 1. (Optional) Fetches repos data from GitHub
/// 2. Loads all training plans from TOML files (avoiding N+1 queries)
/// 3. Determines available course repositories from GitHub or local MDX files
/// 4. Generates course pages with YAML frontmatter, formatting source
///    READMEs in memory for Fumadocs compatibility
/// 5. Builds file trees from worktree.json data
///
/// With --lint, reports MDX issues in fetched READMEs instead of
/// generating pages. Exits non-zero if any errors are found.
#[tokio::main]
async fn main() -> Result<()> {
    // Check for --fetch flag
    let args: Vec<String> = env::args().collect();
    let should_fetch = args.contains(&"--fetch".to_string());
    let lint_pos = args.iter().position(|a| a == "--lint");

    let repo_root = Path::new(".").to_path_buf();
    let repos_dir = repo_root.join("repos");

    // Lint mode: report issues in source files instead of generating.
    // Lints the given file/directory, or ./repos by default.
    if let Some(pos) = lint_pos {
        let target = args
            .get(pos + 1)
            .filter(|a| !a.starts_with("--"))
            .map(|a| Path::new(a).to_path_buf())
            .unwrap_or(repos_dir);

        if !target.exists() {
            eprintln!("Error: lint target not found: {}", target.display());
            std::process::exit(1);
        }

        let (file_count, error_count, warning_count) = linter::lint_path(&target)?;

        if file_count == 0 {
            println!("✓ No issues found");
        } else {
            println!(
                "\n{} error(s), {} warning(s) in {} file(s)",
                error_count, warning_count, file_count
            );
        }

        if error_count > 0 {
            std::process::exit(1);
        }
        return Ok(());
    }

    println!("Repository root: {}", repo_root.display());

    let repos_set = if should_fetch {
        println!("\n=== Fetching repos from GitHub ===");

        let token = fetcher::resolve_github_token().unwrap_or_else(|| {
            eprintln!("Error: No GitHub token found!");
            eprintln!(
                "Please set PERSONAL_ACCESS_TOKEN, GITHUB_TOKEN, or login via `gh auth login`"
            );
            std::process::exit(1);
        });
        let github = fetcher::GitHubFetcher::new(token.clone())?;
        let repo_names = github.list_course_repositories(COURSE_ORG).await?;

        println!("Discovered {} course repositories", repo_names.len());

        // Fetch repos (20 concurrent requests)
        fetcher::fetch_all_repos(token, COURSE_ORG, &repo_names, &repos_dir, 20).await?;

        println!("✓ Repository fetch completed\n");
        repo_names.into_iter().collect()
    } else {
        if !repos_dir.exists() {
            eprintln!("\nError: 'repos' directory not found!");
            eprintln!("This tool requires the repos directory to be populated first.");
            eprintln!("Please run with --fetch flag or ensure repos have been fetched.");
            eprintln!("\nExpected directory: {}", repos_dir.display());
            std::process::exit(1);
        }
        loader::load_local_repo_ids(&repos_dir)?
    };
    println!("Loaded {} course repositories", repos_set.len());

    // Load all training plans from TOML files
    let data_dir = repo_root.join("hoa-major-data");
    let plans = loader::load_all_plans(&data_dir)?;
    println!("Loaded {} training plans", plans.len());

    let shared_categories_config = loader::load_shared_categories(&data_dir);
    if !shared_categories_config.categories.is_empty() {
        println!(
            "Loaded {} shared categories",
            shared_categories_config.categories.len()
        );
    }

    let grades_summary = loader::load_grades_summary(&data_dir);

    let total_courses: usize = plans.iter().map(|p| p.courses.len()).sum();
    println!("Total courses to process: {}", total_courses);

    // Generate course pages
    let docs_dir = repo_root.join("content/docs");
    if !docs_dir.exists() {
        println!("Creating output directory: {}", docs_dir.display());
        fs::create_dir_all(&docs_dir)?;
    }

    println!("Generating course pages...");
    let gen_start = std::time::Instant::now();
    generator::generate_course_pages(
        &plans,
        &shared_categories_config.categories,
        &shared_categories_config.no_course_info_repo_ids,
        &grades_summary,
        &repos_dir,
        &docs_dir,
        &repos_set,
    )
    .await?;
    println!(
        "Course pages generated successfully in {:.2?}",
        gen_start.elapsed()
    );

    println!("\n✓ Done! All pages generated and formatted.");

    Ok(())
}
