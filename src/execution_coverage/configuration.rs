use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};

const WRAPPER: &str = "habeas-coverage";

// {impl}`requirements.test-coverage.setup.configuration`
pub fn workspace_root(root: &Path) -> io::Result<PathBuf> {
    let root = fs::canonicalize(root)?;
    let manifest = root.join("Cargo.toml");
    let parsed: toml::Table = fs::read_to_string(&manifest)?
        .parse()
        .map_err(io::Error::other)?;
    if let Some(workspace) = parsed
        .get("package")
        .and_then(|package| package.get("workspace"))
        .and_then(toml::Value::as_str)
    {
        return fs::canonicalize(root.join(workspace));
    }
    if parsed.contains_key("workspace") {
        return Ok(root);
    }
    for ancestor in root.ancestors().skip(1) {
        let path = ancestor.join("Cargo.toml");
        if path.is_file() {
            let source: toml::Table = fs::read_to_string(path)?
                .parse()
                .map_err(io::Error::other)?;
            if source.contains_key("workspace") {
                if let Some(excludes) = source
                    .get("workspace")
                    .and_then(|workspace| workspace.get("exclude"))
                    .and_then(toml::Value::as_array)
                {
                    for exclude in excludes.iter().filter_map(toml::Value::as_str) {
                        let pattern = glob::Pattern::new(exclude).map_err(io::Error::other)?;
                        if pattern.matches_path_with(
                            root.strip_prefix(ancestor).unwrap(),
                            glob::MatchOptions {
                                require_literal_separator: true,
                                ..Default::default()
                            },
                        ) {
                            return Ok(root);
                        }
                    }
                }
                return Ok(ancestor.to_path_buf());
            }
        }
    }
    Ok(root)
}

fn invalid(path: &Path, message: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!(
        "{}: {message}; repair the coverage integration with `habeas setup` (review conflicting settings first)",
        path.display()
    ))
}

fn command_is_collector(command: &toml::Value) -> bool {
    command.as_str() == Some("habeas coverage-process")
        || command.as_array().is_some_and(|args| {
            args.len() == 2
                && args[0].as_str() == Some("habeas")
                && args[1].as_str() == Some("coverage-process")
        })
}

fn table(item: &mut Item, path: &Path, setting: &str) -> io::Result<()> {
    if item.is_none() {
        *item = Item::Table(Table::new());
    } else if item.is_inline_table() {
        let inline = item.as_inline_table().unwrap().clone();
        *item = Item::Table(inline.into_table());
    } else if !item.is_table() {
        return Err(invalid(path, format!("{setting} must be a table")));
    }
    Ok(())
}

// Deliberately prove an unconditional first run-wrapper rule, rather than approximate
// arbitrary nextest filtersets. Unrelated setup/list rules are left to nextest.
// {impl}`requirements.test-coverage.run.configuration`
pub(super) fn check(source: &str, path: &Path, profile: &str) -> io::Result<()> {
    let config: toml::Table = source.parse().map_err(|error| invalid(path, error))?;
    if !config
        .get("experimental")
        .and_then(toml::Value::as_array)
        .is_some_and(|features| {
            features
                .iter()
                .any(|feature| feature.as_str() == Some("wrapper-scripts"))
        })
    {
        return Err(invalid(path, "experimental must include wrapper-scripts"));
    }
    let wrapper = config
        .get("scripts")
        .and_then(|scripts| scripts.get("wrapper"))
        .and_then(|wrappers| wrappers.get(WRAPPER))
        .and_then(toml::Value::as_table)
        .ok_or_else(|| invalid(path, "scripts.wrapper.habeas-coverage is missing"))?;
    if !wrapper.get("command").is_some_and(command_is_collector)
        || wrapper.get("target-runner").and_then(toml::Value::as_str) != Some("within-wrapper")
    {
        return Err(invalid(
            path,
            "scripts.wrapper.habeas-coverage requires command = [\"habeas\", \"coverage-process\"] and target-runner = \"within-wrapper\"",
        ));
    }
    let profiles = config
        .get("profile")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| invalid(path, "profile.default.scripts is missing"))?;
    // Validate the entire inheritance chain even if an early wrapper rule is
    // unconditional: cycles and an invalid default parent are still malformed.
    let mut parent = profile;
    let mut chain = BTreeSet::new();
    loop {
        if !chain.insert(parent) {
            return Err(invalid(
                path,
                format!("profile.{parent}.inherits forms a cycle"),
            ));
        }
        let settings = profiles.get(parent).and_then(toml::Value::as_table);
        if parent == "default" {
            if settings.is_some_and(|settings| settings.contains_key("inherits")) {
                return Err(invalid(
                    path,
                    "profile.default cannot inherit another profile",
                ));
            }
            break;
        }
        if settings.is_none() && parent != "ci" {
            return Err(invalid(path, format!("profile.{parent} is not defined")));
        }
        parent = match settings.and_then(|settings| settings.get("inherits")) {
            Some(value) => value.as_str().ok_or_else(|| {
                invalid(path, format!("profile.{parent}.inherits must be a string"))
            })?,
            None => "default",
        };
    }
    let mut current = profile;
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(current) {
            return Err(invalid(
                path,
                format!("profile.{current}.inherits forms a cycle"),
            ));
        }
        let settings = profiles.get(current).and_then(toml::Value::as_table);
        if current != "default" && settings.is_none() && current != "ci" {
            return Err(invalid(path, format!("profile.{current} is not defined")));
        }
        if let Some(settings) = settings {
            if let Some(scripts) = settings.get("scripts") {
                let scripts = scripts.as_array().ok_or_else(|| {
                    invalid(path, format!("profile.{current}.scripts must be an array"))
                })?;
                for rule in scripts {
                    let Some(wrapper) = rule.get("run-wrapper") else {
                        continue;
                    };
                    if wrapper.as_str() == Some(WRAPPER)
                        && rule.get("filter").and_then(toml::Value::as_str) == Some("all()")
                        && rule.get("platform").is_none()
                    {
                        return Ok(());
                    }
                    return Err(invalid(
                        path,
                        format!(
                            "profile.{current}.scripts: the first run-wrapper rule must unconditionally select {WRAPPER}; its filter/platform or wrapper prevents proving complete coverage"
                        ),
                    ));
                }
            }
            if current == "default" && settings.contains_key("inherits") {
                return Err(invalid(
                    path,
                    "profile.default cannot inherit another profile",
                ));
            }
        }
        if current == "default" {
            break;
        }
        current = match settings.and_then(|settings| settings.get("inherits")) {
            Some(parent) => parent.as_str().ok_or_else(|| {
                invalid(path, format!("profile.{current}.inherits must be a string"))
            })?,
            None => "default",
        };
    }
    Err(invalid(
        path,
        format!("profile.{profile}.scripts does not route all attempts through {WRAPPER}"),
    ))
}

// {impl}`requirements.test-coverage.setup.validation`
// {impl}`requirements.test-coverage.setup.validation.static`
pub fn validate_setup(root: &Path) -> io::Result<()> {
    let path = workspace_root(root)?.join(".config/nextest.toml");
    let source = fs::read_to_string(&path).map_err(|error| invalid(&path, error))?;
    check(&source, &path, "default")
}

// Returns planned content; callers preflight all conflicts before writing anything.
// {impl}`requirements.test-coverage.setup.configuration`
// {impl}`requirements.test-coverage.setup`
pub fn configure(root: &Path) -> io::Result<(PathBuf, String)> {
    let path = workspace_root(root)?.join(".config/nextest.toml");
    let source = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        String::new()
    };
    let mut config: DocumentMut = source.parse().map_err(|error| invalid(&path, error))?;
    let experimental = &mut config["experimental"];
    if experimental.is_none() {
        *experimental = value(Array::new());
    }
    let features = experimental
        .as_array_mut()
        .ok_or_else(|| invalid(&path, "experimental must be an array"))?;
    if !features
        .iter()
        .any(|feature| feature.as_str() == Some("wrapper-scripts"))
    {
        features.push("wrapper-scripts");
    }
    table(&mut config["scripts"], &path, "scripts")?;
    table(&mut config["scripts"]["wrapper"], &path, "scripts.wrapper")?;
    let wrapper = &mut config["scripts"]["wrapper"][WRAPPER];
    if wrapper.is_none() {
        let mut table = Table::new();
        let mut args = Array::new();
        args.push("habeas");
        args.push("coverage-process");
        table["command"] = value(args);
        table["target-runner"] = value("within-wrapper");
        *wrapper = Item::Table(table);
    }
    table(&mut config["profile"], &path, "profile")?;
    table(&mut config["profile"]["default"], &path, "profile.default")?;
    let scripts = &mut config["profile"]["default"]["scripts"];
    if scripts.is_none() {
        *scripts = Item::ArrayOfTables(ArrayOfTables::new());
    }
    if let Some(array) = scripts.as_array() {
        let mut rules = ArrayOfTables::new();
        for rule in array {
            let rule = rule
                .as_inline_table()
                .ok_or_else(|| invalid(&path, "profile.default.scripts must contain tables"))?;
            rules.push(rule.clone().into_table());
        }
        *scripts = Item::ArrayOfTables(rules);
    }
    let rules = scripts
        .as_array_of_tables_mut()
        .ok_or_else(|| invalid(&path, "profile.default.scripts must be an array of tables"))?;
    if !rules.iter().any(|rule| rule.get("run-wrapper").is_some()) {
        let mut rule = Table::new();
        rule["filter"] = value("all()");
        rule["run-wrapper"] = value(WRAPPER);
        rules.push(rule);
    }
    let desired = config.to_string();
    check(&desired, &path, "default")?;
    Ok((path, desired))
}

#[cfg(test)]
mod tests {
    use super::*;

    // {test}`requirements.test-coverage.setup.configuration`
    // {test}`requirements.test-coverage.setup`
    // {test}`requirements.test-coverage.setup.validation`
    // {test}`requirements.test-coverage.run.configuration`
    #[test]
    fn native_settings_inheritance_and_precedence() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        let (path, source) = configure(temp.path()).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &source).unwrap();
        assert_eq!(configure(temp.path()).unwrap().1, source);
        check(&source, &path, "ci").unwrap();
        let inherited = format!(
            "{source}\n[profile.fast]\ninherits = 'ci'\ntest-threads = 3\n[profile.local]\ninherits = 'fast'\n"
        );
        check(&inherited, &path, "local").unwrap();
        let inherited_conflict = format!(
            "{inherited}\n[[profile.fast.scripts]]\nfilter = 'all()'\nrun-wrapper = 'other'\n"
        );
        assert!(
            check(&inherited_conflict, &path, "local")
                .unwrap_err()
                .to_string()
                .contains("profile.fast.scripts")
        );
        let child_override = format!(
            "{inherited_conflict}\n[[profile.local.scripts]]\nfilter = 'all()'\nrun-wrapper = 'habeas-coverage'\n"
        );
        check(&child_override, &path, "local").unwrap();
        let collector =
            "\n[[profile.local.scripts]]\nfilter = 'all()'\nrun-wrapper = 'habeas-coverage'\n";
        let other = "\n[[profile.local.scripts]]\nfilter = 'test(x)'\nrun-wrapper = 'other'\n";
        check(&format!("{inherited}{collector}{other}"), &path, "local").unwrap();
        assert!(
            check(&format!("{inherited}{other}{collector}"), &path, "local")
                .unwrap_err()
                .to_string()
                .contains("profile.local.scripts")
        );
        for extra in [
            "\n[[profile.local.scripts]]\nfilter = 'test(x)'\nrun-wrapper = 'habeas-coverage'\n",
            "\n[[profile.local.scripts]]\nfilter = 'test(x)'\nrun-wrapper = 'other'\n",
            "\n[[profile.local.scripts]]\nfilter = 'all()'\nplatform = 'cfg(unix)'\nrun-wrapper = 'habeas-coverage'\n",
            "\n[profile.local]\ninherits = 'local'\n",
        ] {
            assert!(check(&format!("{source}{extra}"), &path, "local").is_err());
        }
        assert!(
            check(
                &source.replace("coverage-process", "wrong"),
                &path,
                "default"
            )
            .is_err()
        );
        assert!(
            check(
                &source.replace("within-wrapper", "overrides-wrapper"),
                &path,
                "default"
            )
            .is_err()
        );
        fs::write(&path, "experimental = ['setup-scripts']\n[profile.default]\nretries = 2\n[[profile.default.scripts]]\nfilter = 'all()'\nsetup = 'prepare'\n").unwrap();
        let preserved = configure(temp.path()).unwrap().1;
        check(&preserved, &path, "default").unwrap();
        let settings: toml::Table = preserved.parse().unwrap();
        let experimental = settings["experimental"].as_array().unwrap();
        assert_eq!(experimental.len(), 2);
        assert!(
            experimental
                .iter()
                .any(|entry| entry.as_str() == Some("setup-scripts"))
        );
        assert!(
            experimental
                .iter()
                .any(|entry| entry.as_str() == Some("wrapper-scripts"))
        );
        let profile = &settings["profile"]["default"];
        assert_eq!(profile["retries"].as_integer(), Some(2));
        let scripts = profile["scripts"].as_array().unwrap();
        assert_eq!(scripts.len(), 2);
        assert!(
            scripts
                .iter()
                .any(
                    |entry| entry.get("setup").and_then(toml::Value::as_str) == Some("prepare")
                        && entry["filter"].as_str() == Some("all()")
                )
        );
        assert!(
            scripts.iter().any(
                |entry| entry.get("run-wrapper").and_then(toml::Value::as_str) == Some(WRAPPER)
            )
        );
        fs::write(&path, &preserved).unwrap();
        assert_eq!(configure(temp.path()).unwrap().1, preserved);
        fs::write(
            &path,
            "[[profile.default.scripts]]\nfilter = 'test(x)'\nrun-wrapper = 'custom'\n",
        )
        .unwrap();
        assert!(
            configure(temp.path())
                .unwrap_err()
                .to_string()
                .contains("conflict")
        );
    }

    // {test}`requirements.test-coverage.setup.configuration`
    #[test]
    fn setup_conflicts_name_the_setting_and_preserve_existing_native_configuration() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        let (path, valid) = configure(temp.path()).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for (source, setting) in [
            (
                valid.replace("coverage-process", "other-command"),
                "scripts.wrapper.habeas-coverage",
            ),
            (
                valid.replace("within-wrapper", "overrides-wrapper"),
                "scripts.wrapper.habeas-coverage",
            ),
            (
                "experimental = 'wrong'\n".to_owned(),
                "experimental must be an array",
            ),
            ("scripts = 'wrong'\n".to_owned(), "scripts must be a table"),
            (
                "[profile.default]\nscripts = 'wrong'\n".to_owned(),
                "profile.default.scripts must be an array of tables",
            ),
            (
                "[[profile.default.scripts]]\nfilter = 'test(x)'\nrun-wrapper = 'custom'\n"
                    .to_owned(),
                "profile.default.scripts",
            ),
        ] {
            fs::write(&path, &source).unwrap();
            let error = configure(temp.path()).unwrap_err().to_string();
            assert!(error.contains(setting), "{setting}: {error}");
            assert!(error.contains(".config/nextest.toml"), "{error}");
            assert_eq!(fs::read_to_string(&path).unwrap(), source);
        }
    }

    // {test}`requirements.test-coverage.setup.configuration`
    // {test}`requirements.test-coverage.setup.validation.static`
    #[test]
    fn workspace_member_uses_the_native_config_at_the_workspace_root() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("Cargo.toml"),
            "[workspace]\nmembers = ['member']\n",
        )
        .unwrap();
        let member = temp.path().join("member");
        fs::create_dir(&member).unwrap();
        fs::write(
            member.join("Cargo.toml"),
            "[package]\nname = 'member'\nversion = '0.1.0'\n",
        )
        .unwrap();
        let (path, source) = configure(&member).unwrap();
        assert_eq!(path, temp.path().join(".config/nextest.toml"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
        validate_setup(&member).unwrap();
        assert!(!member.join(".config").exists());
        assert!(!temp.path().join("target").exists());
        fs::write(
            temp.path().join("Cargo.toml"),
            "[workspace]\nmembers = []\nexclude = ['mem*']\n",
        )
        .unwrap();
        let (excluded_path, source) = configure(&member).unwrap();
        assert_eq!(excluded_path, member.join(".config/nextest.toml"));
        fs::create_dir_all(excluded_path.parent().unwrap()).unwrap();
        fs::write(excluded_path, source).unwrap();
        validate_setup(&member).unwrap();
    }

    // {test}`requirements.test-coverage.setup.configuration`
    // {test}`requirements.test-coverage.setup.validation.static`
    #[test]
    fn explicit_workspace_pointer_places_configuration_at_a_sibling_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let member = temp.path().join("member");
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(&member).unwrap();
        fs::write(
            workspace.join("Cargo.toml"),
            "[workspace]\nmembers=['../member']\n",
        )
        .unwrap();
        fs::write(
            member.join("Cargo.toml"),
            "[package]\nname='member'\nversion='0.1.0'\nworkspace='../workspace'\n",
        )
        .unwrap();
        let (path, source) = configure(&member).unwrap();
        assert_eq!(path, workspace.join(".config/nextest.toml"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
        validate_setup(&member).unwrap();
        assert!(!member.join(".config").exists());
        assert!(!workspace.join("target").exists());
    }
}
