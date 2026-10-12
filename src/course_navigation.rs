use crate::constants::parse_semester_folders;
use crate::error::Result;
use crate::models::Plan;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct CourseNavigationArtifact {
    version: u8,
    courses: BTreeMap<String, CourseNavigation>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseNavigation {
    records: Vec<Record>,
    common_assessment_method: Option<String>,
    views: Views,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    entry_year: String,
    major_id: String,
    major_name: String,
    relative_term: String,
    opening_term: String,
    assessment_method: Option<String>,
    href: String,
    #[serde(skip)]
    relative_order: usize,
    #[serde(skip)]
    opening_order: i32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Views {
    by_major: View,
    by_entry_year: View,
    by_opening_term: View,
}

#[derive(Debug, Serialize)]
struct View {
    groups: Vec<Group>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Group {
    key: String,
    label: String,
    assessment_method: Option<String>,
    secondary_label: Option<String>,
    secondary_assessment_method: Option<String>,
    items: Vec<Item>,
    subgroups: Vec<Subgroup>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Subgroup {
    key: String,
    label: String,
    assessment_method: Option<String>,
    items: Vec<Item>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    record_index: usize,
    assessment_method: Option<String>,
}

// Provisional calendar conversion is kept here for the later scheduling refactor.
fn term_labels(entry_year: &str, folder: &str) -> Option<(String, String, usize, i32)> {
    let (stage, season) = folder.split_once('-')?;
    let stage_index = ["fresh", "sophomore", "junior", "senior", "fifth"]
        .iter()
        .position(|name| *name == stage)?;
    let season_index = ["autumn", "spring", "summer"]
        .iter()
        .position(|name| *name == season)?;
    let entry = entry_year
        .parse::<i32>()
        .ok()
        .filter(|year| (1900..=9990).contains(year))?;
    let year = entry + stage_index as i32 + i32::from(season_index != 0);
    let stage_label = ["大一", "大二", "大三", "大四", "大五"][stage_index];
    let relative = format!("{}{}", stage_label, ["上", "下", "夏季"][season_index]);
    let opening = format!("{} 年{}", year, ["秋季", "春季", "夏季"][season_index]);
    let calendar_season = [2, 0, 1][season_index];
    Some((
        relative,
        opening,
        stage_index * 3 + season_index,
        year * 3 + calendar_season,
    ))
}

fn common_assessment<'a>(records: &'a [Record], indexes: &[usize]) -> Option<&'a str> {
    let first = records[*indexes.first()?].assessment_method.as_deref()?;
    indexes
        .iter()
        .all(|index| records[*index].assessment_method.as_deref() == Some(first))
        .then_some(first)
}

#[derive(Clone, Copy)]
enum Axis {
    Major,
    EntryYear,
    OpeningTerm,
}

fn build_view(records: &[Record], axis: Axis, inherited: Option<&str>) -> View {
    let mut primary: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, record) in records.iter().enumerate() {
        let key = match axis {
            Axis::Major => format!("{}\0{}", record.major_name, record.major_id),
            Axis::EntryYear => record.entry_year.clone(),
            Axis::OpeningTerm => record.opening_order.to_string(),
        };
        primary.entry(key).or_default().push(index);
    }
    let mut primary: Vec<_> = primary.into_iter().collect();
    if !matches!(axis, Axis::Major) {
        primary.sort_by(|a, b| {
            b.0.parse::<i32>()
                .unwrap_or(0)
                .cmp(&a.0.parse::<i32>().unwrap_or(0))
        });
    }
    let groups = primary
        .into_iter()
        .map(|(key, indexes)| {
            let record = &records[indexes[0]];
            let label = match axis {
                Axis::Major => record.major_name.clone(),
                Axis::EntryYear => format!("{} 级", record.entry_year),
                Axis::OpeningTerm => record.opening_term.clone(),
            };
            let common = common_assessment(records, &indexes);
            let assessment_method = common
                .filter(|value| Some(*value) != inherited)
                .map(str::to_owned);
            let inherited = common.or(inherited);
            let mut secondary: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
            for index in indexes {
                secondary
                    .entry(records[index].relative_order)
                    .or_default()
                    .push(index);
            }
            let mut subgroups: Vec<_> = secondary
                .into_iter()
                .map(|(order, mut indexes)| {
                    indexes.sort_by(|a, b| {
                        let (a, b) = (&records[*a], &records[*b]);
                        if matches!(axis, Axis::Major) {
                            b.entry_year.cmp(&a.entry_year).then(a.href.cmp(&b.href))
                        } else {
                            a.major_name
                                .cmp(&b.major_name)
                                .then(a.major_id.cmp(&b.major_id))
                                .then(a.href.cmp(&b.href))
                        }
                    });
                    let common = common_assessment(records, &indexes);
                    let assessment_method = common
                        .filter(|value| Some(*value) != inherited)
                        .map(str::to_owned);
                    let inherited = common.or(inherited);
                    let label = records[indexes[0]].relative_term.clone();
                    let items = indexes
                        .into_iter()
                        .map(|index| Item {
                            record_index: index,
                            assessment_method: records[index]
                                .assessment_method
                                .as_deref()
                                .filter(|value| Some(*value) != inherited)
                                .map(str::to_owned),
                        })
                        .collect();
                    Subgroup {
                        key: order.to_string(),
                        label,
                        assessment_method,
                        items,
                    }
                })
                .collect();
            let (secondary_label, secondary_assessment_method, items) = if subgroups.len() == 1 {
                let subgroup = subgroups.remove(0);
                (
                    Some(subgroup.label),
                    subgroup.assessment_method,
                    subgroup.items,
                )
            } else {
                (None, None, Vec::new())
            };
            Group {
                key,
                label,
                assessment_method,
                secondary_label,
                secondary_assessment_method,
                items,
                subgroups,
            }
        })
        .collect();
    View { groups }
}

pub fn build_course_navigation(plans: &[Plan]) -> CourseNavigationArtifact {
    // Match page generation: the last write to the same URL wins.
    let mut by_href: BTreeMap<String, (String, Record)> = BTreeMap::new();
    for plan in plans {
        for course in &plan.courses {
            for (folder, _) in
                parse_semester_folders(course.recommended_semester.as_deref().unwrap_or(""))
            {
                let Some((relative_term, opening_term, relative_order, opening_order)) =
                    term_labels(&plan.year, folder)
                else {
                    continue;
                };
                let href = format!(
                    "/docs/{}/{}/{}/{}",
                    plan.year, plan.major_code, folder, course.repo_id
                );
                let assessment_method = course
                    .assessment_method
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned);
                by_href.insert(
                    href.clone(),
                    (
                        course.repo_id.to_uppercase(),
                        Record {
                            entry_year: plan.year.clone(),
                            major_id: plan.major_code.clone(),
                            major_name: plan.major_name.clone(),
                            relative_term,
                            opening_term,
                            assessment_method,
                            href,
                            relative_order,
                            opening_order,
                        },
                    ),
                );
            }
        }
    }
    let mut courses: BTreeMap<String, Vec<Record>> = BTreeMap::new();
    for (_, (id, record)) in by_href {
        courses.entry(id).or_default().push(record);
    }
    let courses = courses
        .into_iter()
        .map(|(id, records)| {
            let indexes = (0..records.len()).collect::<Vec<_>>();
            let common = common_assessment(&records, &indexes);
            let views = Views {
                by_major: build_view(&records, Axis::Major, common),
                by_entry_year: build_view(&records, Axis::EntryYear, common),
                by_opening_term: build_view(&records, Axis::OpeningTerm, common),
            };
            let common_assessment_method = common.map(str::to_owned);
            (
                id,
                CourseNavigation {
                    records,
                    common_assessment_method,
                    views,
                },
            )
        })
        .collect();
    CourseNavigationArtifact {
        version: 1,
        courses,
    }
}

pub fn write_course_navigation(plans: &[Plan], output: &Path) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, serde_json::to_vec(&build_course_navigation(plans))?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Course, CourseIntroduction, Plan};

    fn plan(year: &str, major: &str, name: &str, assessment: Option<&str>, term: &str) -> Plan {
        Plan {
            year: year.into(),
            major_code: major.into(),
            major_name: name.into(),
            courses: vec![Course {
                repo_id: "CS101".into(),
                name: "课程".into(),
                credit: None,
                assessment_method: assessment.map(str::to_owned),
                course_nature: None,
                recommended_semester: Some(term.into()),
                hours: None,
                total_hours: None,
                grade_details: None,
                introduction: CourseIntroduction::default(),
            }],
        }
    }

    #[test]
    fn preserves_teaching_time_and_excludes_unrecognized_terms() {
        let artifact = build_course_navigation(&[
            plan(
                "2023",
                "A",
                "自动化",
                Some("考试"),
                "第一学年秋季,第二学年春季,第二学年夏季",
            ),
            plan("2024", "B", "计算机科学与技术", None, "未知"),
        ]);
        let records = &artifact.courses["CS101"].records;
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].opening_term, "2023 年秋季");
        assert_eq!(records[1].opening_term, "2025 年春季");
        assert_eq!(records[1].relative_term, "大二下");
        assert_eq!(records[2].relative_term, "大二夏季");
        assert_eq!(records[0].href, "/docs/2023/A/fresh-autumn/CS101");
    }

    #[test]
    fn precomputes_three_views_and_promotes_common_assessment() {
        let artifact = build_course_navigation(&[
            plan("2023", "A", "自动化", Some("考试"), "第二学年秋季"),
            plan("2024", "A", "自动化", Some("考试"), "第二学年秋季"),
        ]);
        let course = &artifact.courses["CS101"];
        assert_eq!(course.common_assessment_method.as_deref(), Some("考试"));
        let group = &course.views.by_major.groups[0];
        assert_eq!(group.secondary_label.as_deref(), Some("大二上"));
        assert!(group.subgroups.is_empty());
        assert_eq!(
            course.records[group.items[0].record_index].entry_year,
            "2024"
        );
        assert!(group
            .items
            .iter()
            .all(|item| item.assessment_method.is_none()));
        assert_eq!(course.views.by_entry_year.groups[0].label, "2024 级");
        assert_eq!(course.views.by_opening_term.groups[0].label, "2025 年秋季");
    }

    #[test]
    fn retains_differing_assessments_on_leaf_items() {
        let artifact = build_course_navigation(&[
            plan("2024", "A", "自动化", Some("考试"), "第二学年秋季"),
            plan("2023", "A", "自动化", Some("考查"), "第二学年秋季"),
            plan("2022", "A", "自动化", None, "第二学年秋季"),
        ]);
        let course = &artifact.courses["CS101"];
        assert!(course.common_assessment_method.is_none());
        let group = &course.views.by_major.groups[0];
        assert!(group.assessment_method.is_none());
        assert_eq!(group.items[0].assessment_method.as_deref(), Some("考试"));
        assert_eq!(group.items[1].assessment_method.as_deref(), Some("考查"));
        assert!(group.items[2].assessment_method.is_none());
    }

    #[test]
    fn hoists_assessment_to_secondary_groups_and_preserves_multiple_terms() {
        let artifact = build_course_navigation(&[
            plan("2024", "A", "自动化", Some("考试"), "第二学年秋季"),
            plan("2023", "A", "自动化", Some("考查"), "第三学年春季"),
        ]);
        let groups = &artifact.courses["CS101"].views.by_major.groups;
        assert_eq!(groups[0].subgroups.len(), 2);
        assert_eq!(
            groups[0].subgroups[0].assessment_method.as_deref(),
            Some("考试")
        );
        assert!(groups[0].subgroups[0].items[0].assessment_method.is_none());
    }

    #[test]
    fn matches_generated_page_overwrites_and_writes_deterministic_json() {
        let plans = vec![
            plan("2024", "A", "自动化", Some("考试"), "第二学年秋季"),
            plan("2024", "A", "自动化", Some("考查"), "第二学年秋季"),
        ];
        let artifact = build_course_navigation(&plans);
        assert_eq!(artifact.courses["CS101"].records.len(), 1);
        assert_eq!(
            artifact.courses["CS101"]
                .common_assessment_method
                .as_deref(),
            Some("考查")
        );
        let path = std::env::temp_dir().join(format!("hoa-navigation-{}.json", std::process::id()));
        write_course_navigation(&plans, &path).unwrap();
        let first = std::fs::read(&path).unwrap();
        write_course_navigation(&plans, &path).unwrap();
        assert_eq!(first, std::fs::read(&path).unwrap());
        std::fs::remove_file(path).unwrap();
    }
}
