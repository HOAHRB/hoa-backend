此仓库为 HOA 前端课程页面生成工具的后端 Rust 实现（项目名 Fuma）。

## 数据源 GitHub 仓库

| 仓库 | 内容 | 用途 |
|------|------|------|
| `HITSZ-OpenAuto/hoa-major-data` | plans、grades_summary、lookup_table、shared_categories | 培养方案、成绩、课程映射 |
| `HITSZ-OpenAuto/repos-management` | repos_list.txt | 可选，过滤课程列表 |
| `HITSZ-OpenAuto/{repo_id}` | README.md（main 分支）、worktree.json（worktree 分支） | 课程内容、文件树元数据 |

## 运行时输入

### 环境变量

使用 `--fetch` 拉取仓库时的可选 GitHub 认证，Token 优先级：

1. `PERSONAL_ACCESS_TOKEN`
2. `GITHUB_TOKEN`
3. `gh auth login`（本地开发）

无 token 时使用未认证请求（60 请求/小时），有 token 时使用认证请求（5000 请求/小时）。

### 文件输入

- `github::hoa-major-data/plans/*.toml` → `hoa-major-data/plans/`（培养方案）
- `github::hoa-major-data/grades_summary.json` → `hoa-major-data/grades_summary.json`（成绩组成）
- `github::hoa-major-data/lookup_table.toml` → `hoa-major-data/lookup_table.toml`（课程代码映射）
- `github::hoa-major-data/shared_categories.toml` → `hoa-major-data/shared_categories.toml`（共享分类）
- `github::repos-management/repos_list.txt` → `repos_list.txt`（可选，过滤课程）
- `HITSZ-OpenAuto/{repo_id}` README.md（main 分支）+ worktree.json（worktree 分支）→ `repos/`（需 `--fetch` 拉取）

---

## 运行时输入文件 schema

### `plans/*.toml`（培养方案）

每行一个 TOML 文件，文件名格式为 `{year}_本_{major_name}.toml`，按年份/专业组织课程数据。参见 [loader.rs](src/loader.rs) 和 [models.rs](src/models.rs)。

**文件结构：**

```toml
[info]
year = "2019"                    # 年份
parent_major_code = "0801"       # 父专业大类代码
parent_major_name = "数学类"      # 父专业大类名称
major_code = "080101"            # 专业代码
major_name = "数据科学与大数据技术"  # 专业名称
school_name = "理学院"           # 所属学院
plan_ID = "BEB8D90..."          # 培养方案 ID（用于 lookup_table 映射）

[[courses]]
course_code = "COMP2021"         # 课程代码（用于 lookup_table 映射）
course_name = "高级语言程序设计"    # 课程名称
credit = 3.0                    # 学分
assessment_method = "考试"       # 考核方式
course_nature = "必修"           # 课程性质
recommended_year_semester = "第一学年秋季"  # 推荐学期
total_hours = 48                # 总学时

[courses.hours]
theory = 28                     # 理论学时
lab = 20                        # 实验学时
practice = 0                    # 实践学时
exercise = 0                    # 习题学时
computer = 0                    # 上机学时
tutoring = 0                    # 答疑学时
```

**字段作用：**

- `course_code`：课程代码，用于 [lookup_table.toml](#lookup_tomltoml-课程代码映射表) 映射到 repo_id
- `credit`：学分，直接传递到前端 frontmatter
- `hours`：学时分布，映射到 frontmatter 的 `hourDistribution`
- `grade_details`：成绩构成（如已在 TOML 中定义则优先使用，否则从 `grades_summary.json` 查找）

---

### `grades_summary.json`（成绩组成）

课程成绩构成明细，按 repo_id 组织，用于补足 TOML 中未定义的 `grade_details`。参见 [loader.rs:37-48](src/loader.rs#L37-L48)。

**数据结构：**

```json
{
  "<repo_id>": {
    "<year>_<major_code>": [
      { "name": "期末考试", "percent": "70%" }
    ],
    "<year>_default": [
      { "name": "期末考试", "percent": "60%" }
    ],
    "default": [
      { "name": "期末考试", "percent": "100%" }
    ]
  }
}
```

**lookup 逻辑（[loader.rs:95-134](src/loader.rs#L95-L134)）：**

1. 先尝试 `{year}_{major_code}` 或 `{year}_{major_name}`（最具体匹配）
2. 再尝试 `{year}_default`（年份默认）
3. 最后 fallback 到 `default`（全局默认）

若均无匹配或详情为空数组，则该课程不显示成绩构成。

---

### `lookup_table.toml`（课程代码映射表）

课程代码到 repo_id 的映射，用于处理同名课程在不同专业中指向不同仓库的情况。参见 [loader.rs:53-85](src/loader.rs#L53-L85)。

**文件结构：**

```toml
[COURSE_CODE]
DEFAULT = "REPO_ID"                    # 所有培养方案通用
"PLAN_ID" = "PLAN_SPECIFIC_REPO_ID"   # 特定培养方案专用
```

**示例：**

```toml
[GEIP1003]
DEFAULT = "GEIP1018"

[MATH1011A]
"3C23C88575EDAD44E0630B18F80AA0F2" = "MATH1015A"
```

**resolve 逻辑（[loader.rs:72-85](src/loader.rs#L72-L85)）：**

1. 精确匹配：`lookup_table[course_code][plan_id]`
2. DEFAULT fallback：`lookup_table[course_code][DEFAULT]`
3. 身份映射：若均无匹配，则 `course_code = repo_id`

---

### `shared_categories.toml`（共享分类）

跨专业共享的课程分类配置。参见 [loader.rs:226-268](src/loader.rs#L226-L268)。

**文件结构：**

```toml
no_course_info_repo_ids = ["GeneralKnowledge", "CrossSpecialty"]

[[categories]]
id = "cross-specialty"
title = "跨专业选修"
repo_ids = ["CrossSpecialty", "Cross-ECON", "Cross-SPST", ...]
```

**字段作用：**

- `no_course_info_repo_ids`：无课程详情页的 repo_id 集合（生成索引页而非课程页）
- `categories`：共享分类列表，每个分类包含 id、title 和 repo_id 列表

---

### `repos/`（课程仓库）

每个课程的 GitHub 仓库内容，需通过 `--fetch` 参数从 GitHub 拉取。目录以 repo_id 命名，每个仓库包含一个 MDX 文件和一个可选的 JSON 文件。参见 [generator.rs:149-195](src/generator.rs#L149-L195)。

**目录结构：**

```
repos/
├── {repo_id}.mdx      # 课程 README 内容（来自 HITSZ-OpenAuto/{repo_id} 的 main 分支）
└── {repo_id}.json     # worktree.json（来自 HITSZ-OpenAuto/{repo_id} 的 worktree 分支）
```

**`{repo_id}.mdx`**：课程 README 的原始内容，来源于各课程仓库的 main 分支。用于提取课程介绍文本。解析时跳过前两行（标题），剩余内容作为页面正文。

**`{repo_id}.json`**：文件树元数据，来源于各课程仓库的 `worktree` 分支，结构如下：

```json
{
  "<文件路径>": {
    "size": 1024,        // 文件大小（字节）
    "time": 1640000000   // Unix 时间戳
  }
}
```

文件树生成规则（[constants.rs:62-97](src/constants.rs#L62-L97)）：

| 排除类型 | 规则 |
|----------|------|
| 文件名 | `.gitkeep`, `README.md`, `LICENSE`, `tag.txt` |
| 扩展名 | `.toml` |
| 目录前缀 | `.github/` |

生成的文件树通过 `tree_to_jsx` 转换为 Fumadocs `<Files>` 组件的 JSX 格式，每条记录生成 `<File>` 或 `<Folder>` 节点。

---

### `repos_list.txt`（可选）

每行一个 repo_id，用于过滤需要处理的课程。如果文件不存在，将处理所有课程。参见 [loader.rs:275-289](src/loader.rs#L275-L289)。

**使用场景：**

- 限制只生成部分课程的页面（常用于开发和测试）
- 与 `shared_categories.toml` 配合过滤 `no_course_info_repo_ids`

---

## 输出结构

```
content/docs/
├── {year}/                      # 年份目录
│   ├── meta.json
│   ├── index.mdx
│   └── {major_code}/            # 专业代码目录
│       ├── meta.json
│       ├── index.mdx
│       └── {semester}/          # 学期目录（如 fresh-autumn）
│           ├── index.mdx
│           └── {repo_id}.mdx    # 课程页面
```

每个 `{repo_id}.mdx` 包含 YAML frontmatter：

```yaml
---
title: 课程名称
description: ""
course:
  credit: 3.0
  assessmentMethod: "考试"
  courseNature: "必修"
  hourDistribution:
    theory: 48
    lab: 0
    practice: 0
    exercise: 0
    computer: 0
    tutoring: 0
  gradingScheme:
    - name: "期末考试"
      percent: 70
---
```

---

## 核心模块

| 模块 | 职责 | 关键函数 |
|------|------|----------|
| `loader` | 加载所有数据文件，预处理避免 N+1 | `load_all_plans`, `load_grades_summary`, `load_lookup_table` |
| `generator` | 生成课程 MDX 页面和文件树 | `generate_course_pages` |
| `formatter` | MDX 格式化（Oxlint 兼容） | `format_all_mdx_files` |
| `fetcher` | GitHub 仓库拉取（`--fetch` 模式） | `fetch_all_repos`（README 来自 main 分支，worktree.json 来自 worktree 分支） |
| `tree` | 文件树生成（解析 worktree.json） | - |
| `models` | 数据结构定义 | `Plan`, `Course`, `Frontmatter` |
