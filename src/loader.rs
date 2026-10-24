//! Data loading utilities for training plans and course information.
//!
//! This module provides functions to load all training plan data from TOML files
//! and attach introductions by original course code. By loading all data
//! upfront, we avoid the N+1 query problem that plagued the Python implementation.

use crate::error::{FumaError, Result};
use crate::models::{Course, CourseIntroduction, GradeDetail, Plan, SharedCategory, TomlPlan};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Debug, Deserialize)]
struct TomlSharedCategories {
    categories: Vec<TomlSharedCategory>,
    #[serde(default)]
    no_course_info_repo_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TomlSharedCategory {
    id: String,
    title: String,
    repo_ids: Vec<String>,
}

/// Grades summary data structure mapping repository IDs to grade details per plan variant
pub type GradesSummary = HashMap<String, HashMap<String, Vec<GradeDetail>>>;
pub type CourseIntroductions = HashMap<String, HashMap<String, CourseIntroduction>>;
/// Lookup table mapping course code to repo ID with optional plan-specific overrides
type LookupTable = HashMap<String, HashMap<String, String>>;

/// Load grades_summary.json if present.
///
/// Returns an empty HashMap if the file doesn't exist or can't be parsed.
pub fn load_grades_summary(data_dir: &Path) -> GradesSummary {
    let path = data_dir.join("grades_summary.json");

    if !path.exists() {
        return HashMap::new();
    }

    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|_| HashMap::new()),
        Err(_) => HashMap::new(),
    }
}

pub fn load_course_introductions(data_dir: &Path) -> Result<CourseIntroductions> {
    let path = data_dir.join("course_introductions.json");
    if !path.exists() {
        return Ok(HashMap::new());
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

/// Load lookup_table.toml if present.
///
/// Returns an empty HashMap if the file doesn't exist or can't be parsed.
fn load_lookup_table(data_dir: &Path) -> LookupTable {
    let path = data_dir.join("lookup_table.toml");

    if !path.exists() {
        return HashMap::new();
    }

    match fs::read_to_string(&path) {
        Ok(content) => toml::from_str(&content).unwrap_or_else(|_| HashMap::new()),
        Err(_) => HashMap::new(),
    }
}

/// Resolve repository ID for a course code by lookup table rules.
///
/// Priority:
/// 1. Exact match by `plan_id`
/// 2. `DEFAULT` fallback
/// 3. Original `course_code` (identity mapping)
fn resolve_repo_id(lookup_table: &LookupTable, course_code: &str, plan_id: &str) -> String {
    lookup_table
        .get(course_code)
        .and_then(|mapping| {
            mapping
                .get(plan_id)
                .or_else(|| mapping.get("DEFAULT"))
                .or_else(|| mapping.get("default"))
        })
        .map(|repo_id| repo_id.trim())
        .filter(|repo_id| !repo_id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| course_code.to_string())
}

/// Load plans with their own grade_details; repository mappings only select resources.
pub fn load_all_plans(data_dir: &Path) -> Result<Vec<Plan>> {
    let plans_dir = data_dir.join("plans");

    if !plans_dir.exists() {
        return Err(FumaError::MissingDirectory(plans_dir));
    }

    let course_introductions = load_course_introductions(data_dir)?;
    // Load course_code -> repo_id lookup table once for all plans
    let lookup_table = load_lookup_table(data_dir);

    let mut plans = Vec::new();

    for entry in WalkDir::new(&plans_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
    {
        let content = fs::read_to_string(entry.path())?;
        let toml_plan: TomlPlan = toml::from_str(&content)?;

        // Preserve curriculum identity independently of shared resource repositories.
        let courses = toml_plan
            .courses
            .into_iter()
            .map(|c| {
                let introduction = course_introductions
                    .get(&c.course_code)
                    .and_then(|variants| variants.get("default"))
                    .cloned()
                    .unwrap_or_default();
                let repo_id =
                    resolve_repo_id(&lookup_table, &c.course_code, &toml_plan.info.plan_id);

                Course {
                    code: c.course_code,
                    repo_id,
                    name: c.course_name,
                    credit: c.credit,
                    assessment_method: c.assessment_method,
                    course_nature: c.course_nature,
                    course_category: c.course_category,
                    offering_college: c.offering_college,
                    recommended_semester: c.recommended_year_semester,
                    hours: c.hours,
                    total_hours: c.total_hours,
                    grade_details: c.grade_details,
                    introduction,
                }
            })
            .collect();

        plans.push(Plan {
            year: toml_plan.info.year,
            major_code: toml_plan.info.major_code,
            major_name: toml_plan.info.major_name,
            courses,
        });
    }

    // Sort plans by year and major_code for deterministic processing
    plans.sort_by(|a, b| a.year.cmp(&b.year).then(a.major_code.cmp(&b.major_code)));

    Ok(plans)
}

/// Config for shared categories and which repo IDs are index pages (no CourseInfo).
pub struct SharedCategoriesConfig {
    pub categories: Vec<SharedCategory>,
    pub no_course_info_repo_ids: HashSet<String>,
}

/// Load shared_categories.toml if present.
///
/// Returns default (empty categories, empty no_course_info set) if file doesn't exist or can't be parsed.
pub fn load_shared_categories(data_dir: &Path) -> SharedCategoriesConfig {
    let path = data_dir.join("shared_categories.toml");

    if !path.exists() {
        return SharedCategoriesConfig {
            categories: Vec::new(),
            no_course_info_repo_ids: HashSet::new(),
        };
    }

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => {
            return SharedCategoriesConfig {
                categories: Vec::new(),
                no_course_info_repo_ids: HashSet::new(),
            };
        }
    };

    let toml: TomlSharedCategories = match toml::from_str(&content) {
        Ok(t) => t,
        Err(_) => {
            return SharedCategoriesConfig {
                categories: Vec::new(),
                no_course_info_repo_ids: HashSet::new(),
            };
        }
    };

    SharedCategoriesConfig {
        categories: toml
            .categories
            .into_iter()
            .map(|c| SharedCategory {
                id: c.id,
                title: c.title,
                repo_ids: c.repo_ids,
            })
            .collect(),
        no_course_info_repo_ids: toml.no_course_info_repo_ids.into_iter().collect(),
    }
}

/// Load repository IDs from local course MDX files.
pub fn load_local_repo_ids(repos_dir: &Path) -> Result<HashSet<String>> {
    if !repos_dir.exists() {
        return Err(FumaError::MissingDirectory(repos_dir.to_path_buf()));
    }

    let mut repo_ids = HashSet::new();
    for entry in fs::read_dir(repos_dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "mdx") {
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                repo_ids.insert(stem.to_string());
            }
        }
    }
    Ok(repo_ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_repo_id_plan_specific() {
        let mut lookup_table = HashMap::new();
        let mut mapping = HashMap::new();
        mapping.insert("PLAN_A".to_string(), "REPO_A".to_string());
        mapping.insert("DEFAULT".to_string(), "REPO_DEFAULT".to_string());
        lookup_table.insert("COURSE1".to_string(), mapping);

        let repo_id = resolve_repo_id(&lookup_table, "COURSE1", "PLAN_A");
        assert_eq!(repo_id, "REPO_A");
    }

    #[test]
    fn test_resolve_repo_id_default_fallback() {
        let mut lookup_table = HashMap::new();
        let mut mapping = HashMap::new();
        mapping.insert("DEFAULT".to_string(), "REPO_DEFAULT".to_string());
        lookup_table.insert("COURSE1".to_string(), mapping);

        let repo_id = resolve_repo_id(&lookup_table, "COURSE1", "PLAN_B");
        assert_eq!(repo_id, "REPO_DEFAULT");
    }

    #[test]
    fn test_resolve_repo_id_identity_fallback() {
        let lookup_table: LookupTable = HashMap::new();

        let repo_id = resolve_repo_id(&lookup_table, "COURSE1", "PLAN_A");
        assert_eq!(repo_id, "COURSE1");
    }

    #[test]
    fn loads_local_repo_ids_from_mdx_files_only() {
        let dir =
            std::env::temp_dir().join(format!("hoa-backend-local-repos-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("CS101.mdx"), "# CS101").unwrap();
        fs::write(dir.join("CS101.json"), "{}").unwrap();
        fs::write(dir.join("ignore.txt"), "x").unwrap();

        let ids = load_local_repo_ids(&dir).unwrap();

        assert_eq!(ids, HashSet::from(["CS101".to_string()]));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_a_missing_local_repositories_directory() {
        let dir =
            std::env::temp_dir().join(format!("hoa-backend-missing-repos-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);

        let error = load_local_repo_ids(&dir).unwrap_err();

        assert!(matches!(error, FumaError::MissingDirectory(path) if path == dir));
    }

    #[test]
    fn test_load_grades_summary_missing_file() {
        use std::env;
        let temp_dir = env::temp_dir().join("test_grades_missing");
        let _ = std::fs::create_dir_all(&temp_dir);

        let result = load_grades_summary(&temp_dir);
        assert!(result.is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_load_grades_summary_valid_file() {
        use std::env;
        let temp_dir = env::temp_dir().join("test_grades_valid");
        let _ = std::fs::create_dir_all(&temp_dir);
        let grades_file = temp_dir.join("grades_summary.json");

        let grades_data = serde_json::json!({
            "MATH101": {
                "2023_CS": [
                    {"name": "Exam", "percent": "70%"}
                ],
                "default": [
                    {"name": "Exam", "percent": "60%"}
                ]
            }
        });

        fs::write(&grades_file, grades_data.to_string()).unwrap();

        let result = load_grades_summary(&temp_dir);

        assert_eq!(result.len(), 1);
        assert!(result.contains_key("MATH101"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_load_grades_summary_invalid_json() {
        use std::env;
        let temp_dir = env::temp_dir().join("test_grades_invalid");
        let _ = std::fs::create_dir_all(&temp_dir);
        let grades_file = temp_dir.join("grades_summary.json");

        fs::write(&grades_file, "invalid json{{{").unwrap();

        let result = load_grades_summary(&temp_dir);

        // Should return empty HashMap on parse error
        assert!(result.is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn load_course_introductions_accepts_bilingual_and_empty_text() {
        let temp_dir = std::env::temp_dir().join("test_course_introductions_valid");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();
        fs::write(
            temp_dir.join("course_introductions.json"),
            r#"{"C001":{"default":{"zh":"中文简介","en":"English"}},"C002":{"default":{"zh":"","en":""}}}"#,
        )
        .unwrap();

        let result = load_course_introductions(&temp_dir).unwrap();

        assert_eq!(result["C001"]["default"].zh, "中文简介");
        assert_eq!(result["C001"]["default"].en, "English");
        assert_eq!(result["C002"]["default"].zh, "");
        fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn load_course_introductions_rejects_present_invalid_json() {
        let temp_dir = std::env::temp_dir().join("test_course_introductions_invalid");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();
        fs::write(temp_dir.join("course_introductions.json"), "not json").unwrap();

        assert!(load_course_introductions(&temp_dir).is_err());
        fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn test_load_lookup_table_missing_file() {
        use std::env;
        let temp_dir = env::temp_dir().join("test_lookup_missing");
        let _ = std::fs::create_dir_all(&temp_dir);

        let result = load_lookup_table(&temp_dir);
        assert!(result.is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_load_lookup_table_valid_file() {
        use std::env;
        let temp_dir = env::temp_dir().join("test_lookup_valid");
        let _ = std::fs::create_dir_all(&temp_dir);
        let lookup_file = temp_dir.join("lookup_table.toml");

        fs::write(
            &lookup_file,
            r#"
[COURSE1]
DEFAULT = "REPO1"

[COURSE2]
PLAN_A = "REPO2A"
"#,
        )
        .unwrap();

        let result = load_lookup_table(&temp_dir);

        assert_eq!(
            result.get("COURSE1").and_then(|m| m.get("DEFAULT")),
            Some(&"REPO1".to_string())
        );
        assert_eq!(
            result.get("COURSE2").and_then(|m| m.get("PLAN_A")),
            Some(&"REPO2A".to_string())
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_load_lookup_table_invalid_toml() {
        use std::env;
        let temp_dir = env::temp_dir().join("test_lookup_invalid");
        let _ = std::fs::create_dir_all(&temp_dir);
        let lookup_file = temp_dir.join("lookup_table.toml");

        fs::write(&lookup_file, "[COURSE1\nDEFAULT = \"BROKEN\"").unwrap();

        let result = load_lookup_table(&temp_dir);
        assert!(result.is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
