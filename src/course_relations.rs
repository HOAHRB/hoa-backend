//! Optional resource relationships. Curriculum offerings remain in plan data.
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::Path,
};

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Relations {
    #[serde(default)]
    groups: BTreeMap<String, Relation>,
    #[serde(default)]
    series: BTreeMap<String, Relation>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    name: String,
    members: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Artifact {
    version: u8,
    groups: BTreeMap<String, Relation>,
    series: BTreeMap<String, Relation>,
    names: BTreeMap<String, String>,
}

fn valid_repository_id(id: &str) -> bool {
    !id.is_empty()
        && id != "."
        && id != ".."
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
}

fn validate(relations: &Relations) -> Result<()> {
    for (id, relation) in relations.groups.iter().chain(&relations.series) {
        let valid_id = id.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
            && id.bytes().all(|c| c.is_ascii_alphanumeric());
        let distinct: BTreeSet<_> = relation.members.iter().collect();
        if !valid_id
            || relation.name.trim().is_empty()
            || distinct.len() < 2
            || distinct.len() != relation.members.len()
            || !relation.members.iter().all(|m| valid_repository_id(m))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid course relation {id}: require PascalCase ID, name and at least two distinct repository IDs"),
            ).into());
        }
    }
    Ok(())
}

pub fn load(path: &Path, repos: &Path) -> Result<Artifact> {
    let relations: Relations = match fs::read_to_string(path) {
        Ok(source) => toml::from_str(&source)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Relations::default(),
        Err(e) => return Err(e.into()),
    };
    validate(&relations)?;
    let mut names = BTreeMap::new();
    for member in relations
        .groups
        .values()
        .chain(relations.series.values())
        .flat_map(|r| &r.members)
    {
        if names.contains_key(member) {
            continue;
        }
        let title = match fs::read_to_string(repos.join(format!("{member}.mdx"))) {
            Ok(source) => crate::generator::title_from_mdx(
                &crate::formatter::format_mdx_file(&source),
                member,
            ),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                eprintln!("Warning: course relation resource {member} has no document; using repository ID");
                member.clone()
            }
            Err(e) => return Err(e.into()),
        };
        let title = if title.trim().is_empty() {
            eprintln!("Warning: course relation resource {member} has no usable title; using repository ID");
            member.clone()
        } else {
            title
        };
        names.insert(member.clone(), title);
    }
    Ok(Artifact {
        version: 1,
        groups: relations.groups,
        series: relations.series,
        names,
    })
}

pub fn write(artifact: &Artifact, output: &Path) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, serde_json::to_vec(artifact)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_series_order_and_many_to_many_membership() {
        let source = "[groups.Example]\nname='Related'\nmembers=['B','A']\n[series.Example]\nname='Sequence'\nmembers=['A','B']";
        let relations: Relations = toml::from_str(source).unwrap();
        validate(&relations).unwrap();
        assert_eq!(relations.series["Example"].members, ["A", "B"]);
        assert_eq!(relations.groups["Example"].members, ["B", "A"]);
    }

    #[test]
    fn rejects_invalid_members_names_and_ids() {
        for source in [
            "[series.Example]\nname='Sequence'\nmembers=['A']",
            "[series.Example]\nname='Sequence'\nmembers=['A','B','A']",
            "[series.Example]\nname='Sequence'\nmembers=['A','../B']",
            "[groups.example]\nname='Related'\nmembers=['A','B']",
            "[groups.Example]\nname=' '\nmembers=['A','B']",
        ] {
            assert!(validate(&toml::from_str(source).unwrap()).is_err());
        }
        assert!(toml::from_str::<Relations>("[tracks]").is_err());
        assert!(toml::from_str::<Relations>(
            "[series.Example]\nname='X'\nmembers=['A','B']\nbranch=['C']"
        )
        .is_err());
    }

    #[test]
    fn missing_file_is_empty_but_invalid_file_is_an_error() {
        let root = std::env::temp_dir().join(format!("hoa-relations-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("course_relations.toml");
        let artifact = load(&root.join("absent.toml"), &root).unwrap();
        assert!(
            artifact.groups.is_empty() && artifact.series.is_empty() && artifact.names.is_empty()
        );
        fs::write(&path, "not valid TOML").unwrap();
        assert!(load(&path, &root).is_err());
        fs::write(root.join("A.mdx"), "# A - Shared name\n\nBody").unwrap();
        fs::write(
            &path,
            "[series.Example]\nname='Sequence'\nmembers=['A','B']",
        )
        .unwrap();
        let artifact = load(&path, &root).unwrap();
        assert_eq!(artifact.names["A"], "Shared name");
        assert_eq!(artifact.names["B"], "B");
        fs::write(root.join("B.mdx"), "title: \"\"\n").unwrap();
        assert_eq!(load(&path, &root).unwrap().names["B"], "B");
        let output = root.join("output.json");
        write(&artifact, &output).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(
            json["series"]["Example"]["members"],
            serde_json::json!(["A", "B"])
        );
        fs::remove_dir_all(root).unwrap();
    }
}
