//! Local game discovery and stdin handoff for Ely.by Play.

use crate::i18n::{tr, trf};
use serde_json::Value;
use std::{
    env, fs,
    net::Ipv6Addr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const VERSION: &str = "26.3";
const JAVA_MAJOR: u64 = 25;
const STORE_RUNTIME: &str =
    "Packages/Microsoft.4297127D64EC6_8wekyb3d8bbwe/LocalCache/Local/runtime";
const MISSING_GAME: &str = "Fichiers de Minecraft 26.3 introuvables ou incomplets. \
    Lancez Minecraft 26.3 une fois dans Prism Launcher ou dans le lanceur officiel, \
    puis relancez ely-play.";

fn metadata(path: &Path) -> Value {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(Value::Null)
}

fn file_part(part: &str) -> bool {
    !part.is_empty()
        && part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}

fn has_assets(root: &Path, manifest: &Value) -> bool {
    let Some(id) = manifest["assetIndex"]["id"]
        .as_str()
        .filter(|id| file_part(id))
    else {
        return false;
    };
    root.join("assets/objects").is_dir()
        && root
            .join("assets/indexes")
            .join(format!("{id}.json"))
            .is_file()
}

fn official_has_game(root: &Path) -> bool {
    let folder = root.join("versions").join(VERSION);
    let manifest = metadata(&folder.join(format!("{VERSION}.json")));
    manifest["id"] == VERSION
        && folder.join(format!("{VERSION}.jar")).is_file()
        && root.join("libraries").is_dir()
        && has_assets(root, &manifest)
}

fn prism_root(instance: &Path) -> Option<&Path> {
    let parent = instance.parent()?;
    if parent
        .file_name()?
        .to_str()?
        .eq_ignore_ascii_case("instances")
    {
        parent.parent()
    } else {
        Some(parent)
    }
}

fn maven_jar(libraries: &Path, coordinate: &str) -> Option<PathBuf> {
    let parts: Vec<_> = coordinate.split(':').collect();
    if !(3..=4).contains(&parts.len())
        || !parts
            .iter()
            .all(|part| file_part(part) && !part.contains(".."))
    {
        return None;
    }
    let mut group = libraries.to_path_buf();
    for part in parts[0].split('.') {
        group.push(part);
    }
    let artifact = parts[1];
    let version = parts[2];
    let suffix = parts
        .get(3)
        .map_or_else(String::new, |part| format!("-{part}"));
    Some(
        group
            .join(artifact)
            .join(version)
            .join(format!("{artifact}-{version}{suffix}.jar")),
    )
}

/// A usable Prism candidate and whether its pack declares a known mod loader.
fn prism_game(instance: &Path) -> Option<bool> {
    let pack = metadata(&instance.join("mmc-pack.json"));
    let components = pack["components"].as_array()?;
    if !components
        .iter()
        .any(|component| component["uid"] == "net.minecraft" && component["version"] == VERSION)
    {
        return None;
    }
    let root = prism_root(instance)?;
    let libraries = root.join("libraries");
    if !libraries.is_dir() {
        return None;
    }
    let folders = [
        instance.join("minecraft/versions").join(VERSION),
        instance.join(".minecraft/versions").join(VERSION),
        instance.join("versions").join(VERSION),
        root.join("versions").join(VERSION),
        root.join("meta/net.minecraft").join(VERSION),
    ];
    let json_name = format!("{VERSION}.json");
    let path = folders
        .iter()
        .map(|folder| folder.join(&json_name))
        .chain([root.join("meta/net.minecraft").join(&json_name)])
        .find(|path| path.is_file())?;
    let manifest = metadata(&path);
    if !has_assets(root, &manifest) {
        return None;
    }
    let jar = format!("{VERSION}.jar");
    let has_jar = [
        libraries.join(format!(
            "com/mojang/minecraft/{VERSION}/minecraft-{VERSION}-client.jar"
        )),
        folders[3].join(&jar),
        folders[0].join(&jar),
        folders[1].join(&jar),
    ]
    .iter()
    .any(|path| path.is_file())
        || manifest["mainJar"]["name"]
            .as_str()
            .and_then(|coordinate| maven_jar(&libraries, coordinate))
            .is_some_and(|path| path.is_file());
    has_jar.then(|| {
        components.iter().any(|component| {
            matches!(
                component["uid"].as_str(),
                Some(
                    "net.fabricmc.fabric-loader"
                        | "org.quiltmc.quilt-loader"
                        | "net.minecraftforge"
                        | "net.neoforged"
                        | "com.mumfrey.liteloader"
                        | "org.lwjgl.rift"
                        | "com.github.terminatornl.liteloaderrift"
                )
            )
        })
    })
}

#[derive(Debug, PartialEq, Eq)]
enum Source {
    Prism(PathBuf),
    Official(PathBuf),
}

impl Source {
    fn prepare_arg(&self) -> (&'static str, &Path) {
        match self {
            Self::Prism(path) => ("--prism-dir", path),
            Self::Official(path) => ("--official-local", path),
        }
    }
}

fn game_sources(
    prism: Option<&Path>,
    official: Option<&Path>,
    appdata: Option<&Path>,
) -> Result<Vec<Source>, String> {
    let mut sources = Vec::new();
    if let Some(path) = prism {
        sources.extend(
            path.canonicalize()
                .ok()
                .filter(|path| prism_game(path).is_some())
                .map(Source::Prism),
        );
    } else if let Some(path) = official {
        sources.extend(
            path.canonicalize()
                .ok()
                .filter(|path| official_has_game(path))
                .map(Source::Official),
        );
    } else if let Some(appdata) = appdata {
        let mut instances: Vec<_> = fs::read_dir(appdata.join("PrismLauncher/instances"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| entry.path().canonicalize().ok())
            .filter_map(|path| Some((prism_game(&path)?, path)))
            .collect();
        instances.sort();
        instances.dedup();
        sources.extend(instances.into_iter().map(|(_, path)| Source::Prism(path)));
        sources.extend(
            appdata
                .join(".minecraft")
                .canonicalize()
                .ok()
                .filter(|path| official_has_game(path))
                .map(Source::Official),
        );
    }
    if sources.is_empty() {
        return Err(tr(MISSING_GAME).into());
    }
    Ok(sources)
}

fn java_version(home: &Path) -> Option<Vec<u64>> {
    let release = fs::read_to_string(home.join("release")).ok()?;
    let version = release
        .lines()
        .find_map(|line| line.strip_prefix("JAVA_VERSION=\"")?.strip_suffix('"'))?;
    if !version.as_bytes().first()?.is_ascii_digit() {
        return None;
    }
    version
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse().ok())
        .collect()
}

fn java_executable(home: &Path) -> PathBuf {
    home.join("bin")
        .join(format!("java{}", env::consts::EXE_SUFFIX))
}

fn managed_java(root: &Path) -> Option<PathBuf> {
    let mut folders = vec![root.to_path_buf()];
    let mut candidates = Vec::new();
    while let Some(home) = folders.pop() {
        if java_executable(&home).is_file() {
            if let Some(version) = java_version(&home).filter(|version| version[0] >= JAVA_MAJOR) {
                candidates.push((version[0] == JAVA_MAJOR, version, home.clone()));
            }
        }
        folders.extend(
            fs::read_dir(&home)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .map(|entry| entry.path()),
        );
    }
    candidates.into_iter().max().map(|(_, _, home)| home)
}

fn checked_java_home(home: &Path) -> Result<PathBuf, String> {
    home.canonicalize()
        .ok()
        .filter(|home| {
            java_executable(home).is_file()
                && java_version(home).is_none_or(|version| version[0] >= JAVA_MAJOR)
        })
        .ok_or_else(|| {
            tr("--java-home doit désigner un dossier Java 25 ou supérieur avec bin/java.").into()
        })
}

fn discover_java(
    source: &Source,
    appdata: Option<&Path>,
    localappdata: Option<&Path>,
) -> Option<PathBuf> {
    let instance_java = match source {
        Source::Prism(instance) => prism_root(instance).map(|root| root.join("java")),
        Source::Official(_) => None,
    };
    instance_java
        .into_iter()
        .chain(appdata.map(|path| path.join("PrismLauncher/java")))
        .chain(appdata.map(|path| path.join(".minecraft/runtime")))
        .chain(localappdata.map(|path| path.join(STORE_RUNTIME)))
        .find_map(|root| managed_java(&root)?.canonicalize().ok())
}

fn local_command(program: &Path, bundle: &Path, java: Option<&Path>) -> Command {
    let mut command = Command::new(program);
    command.current_dir(bundle);
    for name in [
        "HORIZON_SERVER_JAR",
        "HORIZON_MINECRAFT_DIR",
        "HORIZON_SESSION_FILE",
    ] {
        command.env_remove(name);
    }
    if let Some(java) = java {
        command.env("JAVA_HOME", java);
    } else {
        command.env_remove("JAVA_HOME");
    }
    command
}

fn prepare_command(
    program: &Path,
    bundle: &Path,
    resources: &Path,
    source: &Source,
    java: Option<&Path>,
) -> Command {
    let mut command = local_command(program, bundle, java);
    let (source_flag, source_path) = source.prepare_arg();
    command
        .args(["--prepare", "--complete", "--root"])
        .arg(bundle)
        .arg(source_flag)
        .arg(source_path)
        .arg("--output")
        .arg(resources)
        .stdin(Stdio::null());
    if let Some(java) = java {
        command.arg("--java-home").arg(java);
    }
    command
}

fn client_command(program: &Path, bundle: &Path, java: Option<&Path>) -> Command {
    let mut command = local_command(program, bundle, java);
    command
        .env("HORIZON_API_DISCOVERY", "ely.by")
        .env("HORIZON_COSMETICS", "0")
        .env_remove("HORIZON_COSMETICS_API")
        .env_remove("HORIZON_COSMETICS_LOCAL");
    command
}

fn prepare_first<T>(
    sources: &[Source],
    mut prepare: impl FnMut(&Source) -> Result<T, String>,
) -> Result<T, String> {
    let mut failures = Vec::new();
    for source in sources {
        match prepare(source) {
            Ok(result) => return Ok(result),
            Err(error) => failures.push(format!("{}: {error}", source.prepare_arg().1.display())),
        }
    }
    let hint = trf(
        "Échec de la préparation locale. {hint} Vérifiez aussi Java.",
        &[("hint", tr(MISSING_GAME))],
    );
    Err(format!("{hint}\n{}", failures.join("\n")))
}

pub fn is_missing_game(error: &str) -> bool {
    error == tr(MISSING_GAME)
}

pub fn server_address(text: &str) -> Result<String, String> {
    let valid = text.rsplit_once(':').is_some_and(|(host, port)| {
        let dns = host.len() <= 253
            && host
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            && file_part(host);
        let ipv6 = host
            .strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .is_some_and(|host| host.parse::<Ipv6Addr>().is_ok());
        (dns || ipv6)
            && (1..=5).contains(&port.len())
            && port.bytes().all(|b| b.is_ascii_digit())
            && port.parse::<u16>().is_ok_and(|port| port != 0)
    });
    valid
        .then(|| text.to_owned())
        .ok_or_else(|| tr("Indiquez HOST:PORT ou [IPv6]:PORT.").into())
}

pub fn play(
    bundle: &Path,
    prism_instance: Option<&Path>,
    minecraft: Option<&Path>,
    java_override: Option<&Path>,
    server: &str,
) -> Result<i32, String> {
    let bundle = bundle
        .canonicalize()
        .map_err(|_| tr("Indiquez le dossier extrait du client Noelcraft avec --bundle DIR."))?;
    let suffix = env::consts::EXE_SUFFIX;
    let prepare = bundle.join(format!("horizon-prepare{suffix}"));
    let client = bundle.join(format!("horizon-client{suffix}"));
    let lock = metadata(&bundle.join("research-lock.json"));
    if !prepare.is_file() || !client.is_file() || !bundle.join("research-lock.json").is_file() {
        return Err(
            tr("Indiquez le dossier extrait du client Noelcraft avec --bundle DIR.").into(),
        );
    }
    let pinned = &lock["minecraft"];
    if pinned["version"] != VERSION || pinned["java_major"].as_u64() != Some(JAVA_MAJOR) {
        return Err(tr("Cette archive Noelcraft doit cibler Minecraft 26.3 et Java 25.").into());
    }
    let appdata = env::var_os("APPDATA")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    let localappdata = env::var_os("LOCALAPPDATA")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    let sources = game_sources(prism_instance, minecraft, appdata.as_deref())?;
    let java_override = java_override.map(checked_java_home).transpose()?;
    let resources = bundle.join(".local/ely-resources");
    let game_dir = bundle.join(".local/ely-player");
    let java = prepare_first(&sources, |source| {
        let java = java_override
            .clone()
            .or_else(|| discover_java(source, appdata.as_deref(), localappdata.as_deref()));
        let status = prepare_command(&prepare, &bundle, &resources, source, java.as_deref())
            .status()
            .map_err(|e| {
                trf(
                    "Could not start horizon-prepare: {error}",
                    &[("error", &e.to_string())],
                )
            })?;
        if !status.success() {
            return Err(format!("horizon-prepare ({status})"));
        }
        Ok(java)
    })?;
    fs::create_dir_all(&game_dir).map_err(|e| e.to_string())?;

    let session = super::refresh()?.game_session();
    let mut game = client_command(&client, &bundle, java.as_deref())
        .arg("--session-stdin")
        .arg("--resources")
        .arg(resources)
        .arg("--game-dir")
        .arg(&game_dir)
        .arg("--home")
        .arg(&game_dir)
        .arg("--server")
        .arg(server)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| {
            trf(
                "Could not start Noelcraft: {error}",
                &[("error", &e.to_string())],
            )
        })?;
    let mut stdin = game.stdin.take().expect("piped input");
    let result = serde_json::to_writer(&mut stdin, &session);
    drop(stdin);
    if result.is_err() {
        let _ = game.kill();
        let _ = game.wait();
        return Err(tr("Could not send the Ely.by session to Noelcraft").into());
    }
    let status = game.wait().map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        crate::process::supervisor::exit_code(status).map_err(|e| e.to_string())
    }
    #[cfg(not(unix))]
    {
        Ok(status.code().unwrap_or(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn put(root: &Path, name: &str, data: &str) {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, data).unwrap();
    }

    fn game_fixture(appdata: &Path) -> (PathBuf, PathBuf, PathBuf) {
        let official = appdata.join(".minecraft");
        let prism = appdata.join("PrismLauncher");
        let instance = prism.join("instances/Vanilla");
        for root in [&official, &prism] {
            fs::create_dir_all(root.join("libraries")).unwrap();
            fs::create_dir_all(root.join("assets/objects")).unwrap();
            put(root, "assets/indexes/test.json", "{}");
        }
        put(
            &official,
            "versions/26.3/26.3.json",
            r#"{"id":"26.3","assetIndex":{"id":"test"}}"#,
        );
        put(&official, "versions/26.3/26.3.jar", "");
        put(
            &instance,
            "mmc-pack.json",
            r#"{"components":[{"uid":"net.minecraft","version":"26.3"}]}"#,
        );
        put(
            &prism,
            "meta/net.minecraft/26.3.json",
            r#"{"assetIndex":{"id":"test"},"mainJar":{"name":"com.mojang:minecraft:26.3:client"}}"#,
        );
        put(
            &prism,
            "libraries/com/mojang/minecraft/26.3/minecraft-26.3-client.jar",
            "",
        );
        (official, prism, instance)
    }

    fn java_fixture(home: &Path, release: &str) {
        put(home, &format!("bin/java{}", env::consts::EXE_SUFFIX), "");
        put(home, "release", &format!("JAVA_VERSION=\"{release}\"\n"));
    }

    #[test]
    fn prism_wins_then_official_and_an_invalid_override_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let appdata = tmp.path().canonicalize().unwrap();
        let (official, prism, instance) = game_fixture(&appdata);
        assert_eq!(
            game_sources(None, None, Some(&appdata)).unwrap(),
            vec![
                Source::Prism(instance.clone()),
                Source::Official(official.clone())
            ]
        );
        assert_eq!(
            game_sources(None, Some(&official), Some(&appdata)).unwrap(),
            vec![Source::Official(official.clone())]
        );

        fs::remove_file(prism.join("assets/indexes/test.json")).unwrap();
        assert_eq!(
            game_sources(None, None, Some(&appdata)).unwrap(),
            vec![Source::Official(official)]
        );
        assert!(game_sources(Some(&instance), None, Some(&appdata))
            .unwrap_err()
            .contains("Lancez Minecraft 26.3 une fois"));
    }

    #[test]
    fn java_prefers_the_pinned_major_then_the_newest_build() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        for (name, release) in [("a", "25.0.2"), ("b", "25.0.10"), ("c", "26"), ("d", "21")] {
            java_fixture(&root.join(name), release);
        }
        assert_eq!(managed_java(&root), Some(root.join("b")));
        assert_eq!(checked_java_home(&root.join("c")).unwrap(), root.join("c"));
        assert!(checked_java_home(&root.join("d")).is_err());
    }

    #[test]
    fn ely_client_disables_cosmetics_and_clears_overrides() {
        let command = client_command(Path::new("horizon-client"), Path::new("."), None);
        let overrides: std::collections::HashMap<_, _> = command.get_envs().collect();
        assert_eq!(
            overrides.get(OsStr::new("HORIZON_API_DISCOVERY")),
            Some(&Some(OsStr::new("ely.by")))
        );
        assert_eq!(
            overrides.get(OsStr::new("HORIZON_COSMETICS")),
            Some(&Some(OsStr::new("0")))
        );
        for name in [
            "HORIZON_COSMETICS_API",
            "HORIZON_COSMETICS_LOCAL",
            "HORIZON_SESSION_FILE",
        ] {
            assert_eq!(overrides.get(OsStr::new(name)), Some(&None), "{name}");
        }
    }

    #[test]
    fn official_java_roots_and_preparer_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let appdata = root.join("Roaming");
        let localappdata = root.join("Local");
        let source = Source::Official(appdata.join(".minecraft"));
        let discover = || discover_java(&source, Some(&appdata), Some(&localappdata));
        assert_eq!(discover(), None);
        let program = Path::new("horizon-prepare");
        let resources = root.join("resources");
        let command = prepare_command(program, &root, &resources, &source, None);
        assert!(!command.get_args().any(|arg| arg == "--java-home"));
        assert_eq!(
            command.get_envs().find(|(name, _)| *name == "JAVA_HOME"),
            Some((OsStr::new("JAVA_HOME"), None))
        );

        let store = localappdata
            .join(STORE_RUNTIME)
            .join("component/windows-x64/component");
        java_fixture(&store, "25.0.1");
        assert_eq!(discover(), Some(store));
        let official = appdata.join(".minecraft/runtime/component/windows-x64/component");
        java_fixture(&official, "25.0.2");
        assert_eq!(discover(), Some(official.clone()));
        let command = prepare_command(program, &root, &resources, &source, Some(&official));
        assert!(command
            .get_args()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair[0] == "--java-home" && pair[1] == official.as_os_str()));
        assert_eq!(
            command.get_envs().find(|(name, _)| *name == "JAVA_HOME"),
            Some((OsStr::new("JAVA_HOME"), Some(official.as_os_str())))
        );
    }

    #[test]
    fn preparation_prefers_vanilla_and_retries_through_official() {
        let tmp = tempfile::tempdir().unwrap();
        let appdata = tmp.path().canonicalize().unwrap();
        let (official, prism, vanilla) = game_fixture(&appdata);
        let modded = prism.join("instances/A Fabric");
        put(
            &modded,
            "mmc-pack.json",
            r#"{"components":[{"uid":"net.minecraft","version":"26.3"},{"uid":"net.fabricmc.fabric-loader"}]}"#,
        );

        let sources = game_sources(None, None, Some(&appdata)).unwrap();
        assert_eq!(
            sources,
            vec![
                Source::Prism(vanilla.clone()),
                Source::Prism(modded.clone()),
                Source::Official(official.clone()),
            ]
        );
        let mut attempted = Vec::new();
        let prepared = prepare_first(&sources, |source| {
            attempted.push(source.prepare_arg().1.to_path_buf());
            match source {
                Source::Prism(_) => Err("fixture preparation failed".into()),
                Source::Official(_) => Ok("official"),
            }
        })
        .unwrap();
        assert_eq!(prepared, "official");
        assert_eq!(attempted, vec![vanilla, modded.clone(), official]);

        let explicit = game_sources(Some(&modded), None, Some(&appdata)).unwrap();
        assert_eq!(explicit, vec![Source::Prism(modded)]);
        let error = prepare_first(&explicit, |_| {
            Err::<(), _>("fixture preparation failed".into())
        })
        .unwrap_err();
        assert!(error.contains("fixture preparation failed"));
    }
}
