"""Préparer le client Windows Noelcraft et rejoindre un serveur avec Ely.by."""

import argparse
import getpass
import ipaddress
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import sys
import urllib.error
import urllib.request
import uuid
import warnings

AUTH = "https://authserver.ely.by/auth/authenticate"
VERSION = "26.3"
PREPARATION_FAILED = (
    "La préparation a échoué : fichiers de Minecraft 26.3 incomplets ou téléchargement coupé. "
    "Relancez le script, les octets déjà reçus sont repris. Vérifiez aussi Java."
)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        return None


def read_metadata(path):
    try:
        value = json.loads(path.read_bytes())
    except (OSError, ValueError):
        return {}
    return value if isinstance(value, dict) else {}


def has_assets(root):
    return all((root / "assets" / name).is_dir() for name in ("indexes", "objects"))


def official_has_game(root):
    version = root / "versions" / VERSION
    manifest = read_metadata(version / f"{VERSION}.json")
    index = manifest.get("assetIndex")
    index_id = index.get("id") if isinstance(index, dict) else None
    return (
        manifest.get("id") == VERSION
        and (version / f"{VERSION}.jar").is_file()
        and (root / "libraries").is_dir()
        and has_assets(root)
        and isinstance(index_id, str)
        and re.fullmatch(r"[A-Za-z0-9_.-]+", index_id)
        and (root / "assets" / "indexes" / f"{index_id}.json").is_file()
    )


def prism_root(instance):
    if instance.parent.name.lower() == "instances":
        return instance.parent.parent
    return instance.parent


def prism_has_game(instance):
    components = read_metadata(instance / "mmc-pack.json").get("components")
    if not isinstance(components, list) or not any(
        isinstance(component, dict)
        and component.get("uid") == "net.minecraft"
        and component.get("version") == VERSION
        for component in components
    ):
        return False
    root = prism_root(instance)
    if not (root / "libraries").is_dir() or not has_assets(root):
        return False

    version_folders = [
        instance / "minecraft" / "versions" / VERSION,
        instance / ".minecraft" / "versions" / VERSION,
        instance / "versions" / VERSION,
        root / "versions" / VERSION,
        root / "meta" / "net.minecraft" / VERSION,
    ]
    manifests = [folder / f"{VERSION}.json" for folder in version_folders]
    manifests.append(root / "meta" / "net.minecraft" / f"{VERSION}.json")
    manifest_path = next((path for path in manifests if path.is_file()), None)
    if manifest_path is None:
        return False
    manifest = read_metadata(manifest_path)
    index = manifest.get("assetIndex")
    index_id = index.get("id") if isinstance(index, dict) else None
    if not (
        isinstance(index_id, str)
        and re.fullmatch(r"[A-Za-z0-9_.-]+", index_id)
        and (root / "assets" / "indexes" / f"{index_id}.json").is_file()
    ):
        return False

    jars = [
        root / "libraries" / "com" / "mojang" / "minecraft" / VERSION / f"minecraft-{VERSION}-client.jar",
        version_folders[3] / f"{VERSION}.jar",
        version_folders[0] / f"{VERSION}.jar",
        version_folders[1] / f"{VERSION}.jar",
    ]
    main_jar = manifest.get("mainJar")
    coordinate = main_jar.get("name") if isinstance(main_jar, dict) else None
    if isinstance(coordinate, str):
        parts = coordinate.split(":")
        if len(parts) in (3, 4) and all(
            re.fullmatch(r"[A-Za-z0-9_.-]+", part) and ".." not in part
            for part in parts
        ):
            group, artifact, version = parts[:3]
            classifier = f"-{parts[3]}" if len(parts) == 4 else ""
            jars.append(
                root.joinpath("libraries", *group.split("."), artifact, version,
                              f"{artifact}-{version}{classifier}.jar")
            )
    return any(path.is_file() for path in jars)


def game_source(args, appdata):
    # Un dossier nommé explicitement sert toujours : le téléchargement complète ce qui y manque.
    if args.prism_instance is not None:
        return "--prism-dir", args.prism_instance.resolve()
    if args.minecraft is not None:
        return "--official-local", args.minecraft.resolve()
    if appdata is not None:
        instances = appdata / "PrismLauncher" / "instances"
        if instances.is_dir():
            for instance in sorted(instances.iterdir()):
                if prism_has_game(instance):
                    return "--prism-dir", instance
        minecraft = appdata / ".minecraft"
        if official_has_game(minecraft):
            return "--official-local", minecraft
    # Aucun dossier : le préparateur télécharge chaque fichier épinglé.
    return None, None


def java_version(home):
    try:
        release = (home / "release").read_text(encoding="utf-8")
    except (OSError, UnicodeError):
        return None
    match = re.search(r'^JAVA_VERSION="(\d+)([^"]*)"$', release, re.MULTILINE)
    if not match:
        return None
    return (int(match[1]), *(int(part) for part in re.findall(r"\d+", match[2])))


def java_home(override, managed_roots, minimum):
    if override is not None:
        home = override.resolve()
        if (home / "bin" / "java.exe").is_file():
            version = java_version(home)
            if version is None or version[0] >= minimum:
                return home
        raise ValueError(f"--java-home doit désigner un dossier Java {minimum} ou supérieur avec bin\\java.exe.")
    for root in dict.fromkeys(managed_roots):
        if not root.is_dir():
            continue
        candidates = []
        for executable in root.glob("**/bin/java.exe"):
            if executable.is_file():
                home = executable.parent.parent
                version = java_version(home)
                if version and version[0] >= minimum:
                    candidates.append((version, home))
        if candidates:
            return max(candidates, key=lambda item: (item[0][0] == minimum, item[0], str(item[1])))[1]
    return None


def server_address(text):
    host, separator, port = text.rpartition(":")
    valid_host = bool(re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,252}", host))
    if host.startswith("[") and host.endswith("]"):
        try:
            ipaddress.IPv6Address(host[1:-1])
            valid_host = True
        except ValueError:
            pass
    if not (separator and valid_host and re.fullmatch(r"[0-9]{1,5}", port)
            and 1 <= int(port) <= 65535):
        raise argparse.ArgumentTypeError("indiquez HOST:PORT ou [IPv6]:PORT")
    return text


def authenticate(username, password, client_token):
    payload = json.dumps({
        "username": username,
        "password": password,
        "clientToken": client_token,
        "requestUser": False,
    }).encode("utf-8")
    request = urllib.request.Request(
        AUTH, payload, {"Content-Type": "application/json"}, method="POST"
    )
    opener = urllib.request.build_opener(NoRedirect())
    try:
        with opener.open(request, timeout=15) as response:
            raw = response.read(65537)
            if response.status != 200 or len(raw) > 65536:
                raise ValueError("Réponse de connexion Ely.by inattendue.")
            try:
                return json.loads(raw)
            except ValueError:
                raise ValueError("Réponse de connexion Ely.by invalide.") from None
    except urllib.error.HTTPError as error:
        with error:
            if error.code == 401:
                try:
                    data = json.loads(error.read(16384))
                except ValueError:
                    data = {}
                if (
                    isinstance(data, dict)
                    and data.get("error") == "ForbiddenOperationException"
                    and data.get("errorMessage") == "Account protected with two factor auth."
                ):
                    return None
        raise ValueError(f"Connexion Ely.by refusée (HTTP {error.code}).") from None


def session_from(reply, client_token):
    if not isinstance(reply, dict) or reply.get("clientToken") != client_token:
        raise ValueError("Réponse de connexion Ely.by invalide.")
    profile = reply.get("selectedProfile")
    if not isinstance(profile, dict):
        raise ValueError("Ely.by n’a renvoyé aucun profil sélectionné.")
    name = profile.get("name")
    identity = profile.get("id")
    token = reply.get("accessToken")
    if not (
        isinstance(name, str)
        and re.fullmatch(r"[A-Za-z0-9_]{1,16}", name)
        and isinstance(identity, str)
        and re.fullmatch(r"(?:[0-9a-fA-F]{32}|[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12})", identity)
        and isinstance(token, str)
        and token
    ):
        raise ValueError("Session de jeu Ely.by invalide.")
    player_id = uuid.UUID(identity)
    if not player_id.int:
        raise ValueError("UUID Ely.by invalide.")
    return {"name": name, "uuid": player_id.hex, "access_token": token, "user_type": "ely"}


def login():
    username = getpass.getpass("Pseudo ou adresse e-mail Ely.by : ").strip()
    if not username:
        raise ValueError("Saisissez un pseudo ou une adresse e-mail Ely.by.")
    password = getpass.getpass("Mot de passe Ely.by : ")
    client_token = secrets.token_hex(16)
    reply = authenticate(username, password, client_token)
    if reply is None:
        code = getpass.getpass("Code TOTP Ely.by : ")
        if not re.fullmatch(r"[0-9]{6}", code):
            raise ValueError("Saisissez un code TOTP à six chiffres.")
        reply = authenticate(username, f"{password}:{code}", client_token)
        if reply is None:
            raise ValueError("Ely.by demande toujours un code TOTP.")
    return session_from(reply, client_token)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, default=Path(__file__).resolve().parent)
    sources = parser.add_mutually_exclusive_group()
    sources.add_argument("--prism-instance", type=Path,
                         help="instance Prism à compléter par téléchargement")
    sources.add_argument("--minecraft", type=Path,
                         help="dossier de jeu officiel à compléter par téléchargement")
    parser.add_argument("--java-home", type=Path)
    parser.add_argument("--server", required=True, type=server_address)
    args = parser.parse_args()

    bundle = args.bundle.resolve()
    prepare = bundle / "horizon-prepare.exe"
    client = bundle / "horizon-client.exe"
    lock = read_metadata(bundle / "research-lock.json").get("minecraft")
    if not all(path.is_file() for path in (prepare, client, bundle / "research-lock.json")):
        raise ValueError("Indiquez le dossier extrait du client Windows Noelcraft avec --bundle DIR.")
    if not isinstance(lock, dict) or lock.get("version") != VERSION or lock.get("java_major") != 25:
        raise ValueError("Cette archive Noelcraft doit cibler Minecraft 26.3 et Java 25.")

    appdata = Path(os.environ["APPDATA"]) if os.environ.get("APPDATA") else None
    source_flag, source = game_source(args, appdata)
    roots = []
    if source is not None:
        roots.append(prism_root(source) / "java" if source_flag == "--prism-dir" else source / "runtime")
    if appdata is not None:
        roots += [appdata / "PrismLauncher" / "java", appdata / ".minecraft" / "runtime"]
    if os.environ.get("LOCALAPPDATA"):
        roots.append(Path(os.environ["LOCALAPPDATA"]) / "Packages" / "Microsoft.4297127D64EC6_8wekyb3d8bbwe"
                     / "LocalCache" / "Local" / "runtime")
    # Without a Java found here, the preparer looks for one itself (the official launcher's runtimes).
    java = java_home(args.java_home, roots, lock["java_major"])
    if sys.stdin is None or not sys.stdin.isatty():
        raise ValueError("Lancez ce script dans PowerShell ou le Terminal Windows : "
                         "la saisie masquée du mot de passe a besoin d'une vraie console.")

    env = os.environ.copy()
    for name in ("HORIZON_SERVER_JAR", "HORIZON_MINECRAFT_DIR", "HORIZON_SESSION_FILE"):
        env.pop(name, None)
    # Only an explicit --java-home replaces JAVA_HOME; a Java found here is offered to the preparer as one more
    # candidate, after JAVA_HOME and PATH.
    if args.java_home is not None:
        env["JAVA_HOME"] = str(java)
    resources = bundle / ".local" / "ely-resources"
    game = bundle / ".local" / "ely-player"
    subprocess.run(
        [
            prepare, "--root", bundle, "--output", resources, "--prepare", "--complete",
            *([source_flag, str(source)] if source is not None else []),
            *(["--java-home", java] if java is not None else []),
        ],
        cwd=bundle, env=env, stdin=subprocess.DEVNULL, check=True,
    )
    game.mkdir(parents=True, exist_ok=True)
    with warnings.catch_warnings():
        warnings.simplefilter("error", getpass.GetPassWarning)
        session = login()
    env["HORIZON_API_DISCOVERY"] = "ely.by"
    env["HORIZON_COSMETICS"] = "0"
    return subprocess.run(
        [
            client, "--session-stdin", "--resources", resources,
            "--game-dir", game, "--home", game, "--server", args.server,
        ],
        cwd=bundle, env=env, input=json.dumps(session).encode("utf-8"),
        check=False,
    ).returncode


if __name__ == "__main__":
    try:
        sys.exit(main())
    except subprocess.CalledProcessError:
        sys.exit(PREPARATION_FAILED)
    except (getpass.GetPassWarning, EOFError):
        sys.exit("Utilisez un terminal interactif qui masque la saisie.")
    except KeyboardInterrupt:
        sys.exit("Connexion annulée.")
    except (urllib.error.URLError, TimeoutError):
        sys.exit("Le service de connexion Ely.by est indisponible.")
    except OSError:
        sys.exit("Erreur d’accès aux fichiers ou au programme Windows.")
    except ValueError as error:
        sys.exit(str(error))
