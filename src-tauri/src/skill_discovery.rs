use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub source: String,
}

#[derive(Debug, Clone)]
struct SkillCacheEntry {
    info: SkillInfo,
    skill_md_path: PathBuf,
}

/// Cache keyed by `project_dir` (None = global-only scan). Populated lazily on
/// first `discover_skills` and cleared by `skill_refresh`.
static SKILL_CACHE: Mutex<Option<HashMap<Option<String>, Vec<SkillCacheEntry>>>> = Mutex::new(None);

/// Max bytes we'll inline as a skill body. Prevents a fat skill from blowing
/// the executor's context budget. Matches `MAX_FILE_SIZE` shape in file_cache.
const MAX_SKILL_BODY_BYTES: u64 = 32 * 1024;

#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
}

fn parse_skill_md(path: &Path) -> Option<SkillInfo> {
    let content = fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = content.lines().collect();

    if lines.first()? != &"---" {
        return None;
    }

    let end = lines.iter().skip(1).position(|&line| line == "---")?;
    let yaml_content = lines[1..=end].join("\n");

    let frontmatter: SkillFrontmatter = serde_yaml::from_str(&yaml_content).ok()?;

    Some(SkillInfo {
        name: frontmatter.name,
        description: frontmatter.description,
        source: path.parent()?.to_string_lossy().to_string(),
    })
}

fn scan_skills_dir(dir: &Path) -> Vec<SkillCacheEntry> {
    let mut skills = Vec::new();

    let Ok(entries) = fs::read_dir(dir) else {
        return skills;
    };

    for entry in entries.flatten() {
        // `file_type` doesn't follow symlinks; resolve via metadata so the
        // user's symlinked ~/.claude/skills → ~/.agents/skills works.
        let is_dir = entry
            .metadata()
            .map(|m| m.is_dir())
            .unwrap_or(false);
        if !is_dir {
            continue;
        }
        let skill_md = entry.path().join("SKILL.md");
        if !skill_md.exists() {
            continue;
        }
        if let Some(info) = parse_skill_md(&skill_md) {
            skills.push(SkillCacheEntry {
                info,
                skill_md_path: skill_md,
            });
        }
    }

    skills
}

fn scan_all(project_dir: Option<&str>) -> Vec<SkillCacheEntry> {
    let mut acc: Vec<SkillCacheEntry> = Vec::new();
    // First hit wins: project-local roots shadow global ones, and earlier
    // entries in each list win over later ones.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut push_unique = |scanned: Vec<SkillCacheEntry>, acc: &mut Vec<SkillCacheEntry>| {
        for entry in scanned {
            if seen.insert(entry.info.name.clone()) {
                acc.push(entry);
            }
        }
    };

    // Project-local roots take precedence.
    if let Some(dir) = project_dir {
        let project_path = PathBuf::from(dir);
        for subdir in [".opencode/skills", ".claude/skills", ".agents/skills"] {
            push_unique(scan_skills_dir(&project_path.join(subdir)), &mut acc);
        }
    }

    if let Some(home) = dirs::home_dir() {
        for subdir in [
            ".config/opencode/skills",
            ".claude/skills",
            ".claude/agents",
            ".agents/skills",
        ] {
            push_unique(scan_skills_dir(&home.join(subdir)), &mut acc);
        }
    }

    acc.sort_by(|a, b| a.info.name.cmp(&b.info.name));
    acc
}

fn cache_or_populate(project_dir: Option<&str>) -> Vec<SkillCacheEntry> {
    let key = project_dir.map(|s| s.to_string());

    {
        let guard = SKILL_CACHE.lock().expect("skill cache poisoned");
        if let Some(map) = guard.as_ref() {
            if let Some(cached) = map.get(&key) {
                return cached.clone();
            }
        }
    }

    let scanned = scan_all(project_dir);

    {
        let mut guard = SKILL_CACHE.lock().expect("skill cache poisoned");
        let map = guard.get_or_insert_with(HashMap::new);
        map.insert(key, scanned.clone());
    }

    scanned
}

fn clear_cache(project_dir: Option<&str>) {
    let mut guard = SKILL_CACHE.lock().expect("skill cache poisoned");
    if let Some(map) = guard.as_mut() {
        map.remove(&project_dir.map(|s| s.to_string()));
    }
}

fn discover_skills_impl(project_dir: Option<&str>) -> Vec<SkillInfo> {
    cache_or_populate(project_dir)
        .into_iter()
        .map(|e| e.info)
        .collect()
}

fn read_content_impl(name: &str, project_dir: Option<&str>) -> Result<String, String> {
    let entries = cache_or_populate(project_dir);
    let entry = entries
        .iter()
        .find(|e| e.info.name == name)
        .ok_or_else(|| format!("Skill not found: {}", name))?;

    let metadata = fs::metadata(&entry.skill_md_path)
        .map_err(|e| format!("Failed to read skill metadata: {}", e))?;
    if metadata.len() > MAX_SKILL_BODY_BYTES {
        return Err(format!(
            "Skill body too large ({} bytes). Maximum is {} bytes.",
            metadata.len(),
            MAX_SKILL_BODY_BYTES
        ));
    }

    fs::read_to_string(&entry.skill_md_path)
        .map_err(|e| format!("Failed to read skill: {}", e))
}

#[tauri::command]
pub fn discover_skills(project_dir: Option<String>) -> Vec<SkillInfo> {
    discover_skills_impl(project_dir.as_deref())
}

#[tauri::command]
pub fn skill_read_content(
    name: String,
    project_dir: Option<String>,
) -> Result<String, String> {
    read_content_impl(&name, project_dir.as_deref())
}

#[tauri::command]
pub fn skill_refresh(project_dir: Option<String>) -> Vec<SkillInfo> {
    clear_cache(project_dir.as_deref());
    discover_skills_impl(project_dir.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_skill(dir: &Path, name: &str, description: &str, body: &str) {
        let skill_dir = dir.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        let content = format!(
            "---\nname: {}\ndescription: {}\n---\n\n{}",
            name, description, body
        );
        fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    fn unique_root(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("the-pair-skill-test-{}-{}-{}", label, std::process::id(), nanos))
    }

    #[test]
    fn parse_skill_md_reads_frontmatter() {
        let root = unique_root("parse");
        write_skill(&root, "demo-skill", "Does demo things", "body content");
        let info = parse_skill_md(&root.join("demo-skill").join("SKILL.md")).unwrap();
        assert_eq!(info.name, "demo-skill");
        assert_eq!(info.description, "Does demo things");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn scan_skills_dir_skips_folders_without_skill_md() {
        let root = unique_root("scan");
        fs::create_dir_all(&root).unwrap();
        write_skill(&root, "valid", "ok", "body");
        // Empty folder — should be skipped.
        fs::create_dir_all(&root.join("not-a-skill")).unwrap();

        let scanned = scan_skills_dir(&root);
        assert_eq!(scanned.len(), 1);
        assert_eq!(scanned[0].info.name, "valid");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn read_content_returns_body_and_enforces_size_cap() {
        // Use isolated cache key so concurrent tests don't collide.
        let root = unique_root("body");
        write_skill(&root, "small-skill", "small", "tiny body");
        write_skill(
            &root,
            "big-skill",
            "huge",
            &"x".repeat((MAX_SKILL_BODY_BYTES as usize) + 16),
        );

        // Force the project-local roots to point at our test dir by faking the
        // ".claude/skills" sub-path the impl expects.
        let project = unique_root("project");
        let claude_root = project.join(".claude").join("skills");
        fs::create_dir_all(&claude_root).unwrap();
        // Copy the SKILL.md files into the conventional location.
        for name in ["small-skill", "big-skill"] {
            let src = root.join(name);
            let dest = claude_root.join(name);
            fs::create_dir_all(&dest).unwrap();
            fs::copy(src.join("SKILL.md"), dest.join("SKILL.md")).unwrap();
        }

        // Clear any leftover cache for this key.
        clear_cache(Some(project.to_string_lossy().as_ref()));

        let body = read_content_impl("small-skill", Some(project.to_string_lossy().as_ref()))
            .expect("small body reads");
        assert!(body.contains("tiny body"));

        let err = read_content_impl("big-skill", Some(project.to_string_lossy().as_ref()))
            .expect_err("oversized body rejected");
        assert!(err.contains("too large"));

        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(&project).unwrap();
    }

    #[test]
    fn project_roots_shadow_global_duplicates_by_name() {
        let project = unique_root("shadow");
        let project_skill_dir = project.join(".claude").join("skills");
        fs::create_dir_all(&project_skill_dir).unwrap();
        write_skill(&project_skill_dir, "shared", "from-project", "project body");

        clear_cache(Some(project.to_string_lossy().as_ref()));
        let skills = discover_skills_impl(Some(project.to_string_lossy().as_ref()));
        let entry = skills
            .iter()
            .find(|s| s.name == "shared")
            .expect("project skill present");
        assert_eq!(entry.description, "from-project");

        fs::remove_dir_all(&project).unwrap();
    }
}
