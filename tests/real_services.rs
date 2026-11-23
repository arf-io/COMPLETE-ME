//! Opt-in tests for actual local runtime contracts and Linux help isolation.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

struct Scratch(PathBuf);

impl Scratch {
    fn new(parent: &Path, label: &str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = parent.join(format!("cm-{label}-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn checked(out: Output) -> String {
    assert!(
        out.status.success(),
        "status {}\nstdout {}\nstderr {}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

struct ComposeProject<'a> {
    dir: &'a Path,
    name: String,
    socket: String,
}

impl ComposeProject<'_> {
    fn docker(&self) -> Command {
        let mut cmd = Command::new("docker");
        cmd.current_dir(self.dir)
            .env("DOCKER_HOST", &self.socket)
            .env_remove("DOCKER_CONTEXT");
        cmd
    }

    fn compose(&self, args: &[&str]) -> Command {
        let mut cmd = self.docker();
        cmd.args(["compose", "--project-name", &self.name, "--file", "compose.json"])
            .args(args);
        cmd
    }

    fn live(&self, args: &[&str], prefix: &str) -> String {
        checked(
            Command::new(env!("CARGO_BIN_EXE_complete-me"))
                .env("DOCKER_HOST", &self.socket)
                .env_remove("DOCKER_CONTEXT")
                .env_remove("COMPLETE_ME_NO_REMOTE")
                .env("XDG_CACHE_HOME", self.dir.join("cache"))
                .current_dir(self.dir)
                .args(["live", "--current", prefix, "--", "docker"])
                .args(args)
                .output()
                .unwrap(),
        )
    }
}

impl Drop for ComposeProject<'_> {
    fn drop(&mut self) {
        let _ = self.compose(&["down", "--volumes", "--timeout", "1"]).output();
    }
}

#[test]
fn docker_and_compose_follow_real_container_state() {
    if std::env::var_os("COMPLETE_ME_TEST_DOCKER").is_none() {
        eprintln!("opt in with COMPLETE_ME_TEST_DOCKER=1 to test Docker and Compose");
        return;
    }
    let dir = Scratch::new(&std::env::temp_dir(), "docker");
    let name = format!("cm-e2e-{}", dir.path().file_name().unwrap().to_string_lossy());
    let socket = std::env::var("COMPLETE_ME_TEST_DOCKER_HOST").expect("provide the local Docker socket");
    assert!(socket.starts_with("unix://"), "use a local disposable Docker daemon");
    let project = ComposeProject {
        dir: dir.path(),
        name,
        socket,
    };
    checked(
        project
            .docker()
            .args(["info", "--format", "{{.ServerVersion}}"])
            .output()
            .unwrap(),
    );
    assert!(project.live(&["container"], "lo").contains("logs\t"));
    assert!(project.live(&["container", "logs"], "--tai").contains("--tail\t"));
    let image = std::env::var("COMPLETE_ME_TEST_DOCKER_IMAGE").unwrap_or_else(|_| "busybox:1.36.1".into());
    std::fs::write(
        dir.path().join("compose.json"),
        serde_json::json!({
            "services": {
                "worker": {"image": image, "command": ["sleep", "120"], "stop_grace_period": "1s",
                    "restart": "on-failure:3", "volumes": ["state:/state"]},
                "debugger": {"image": image, "profiles": ["debug"], "command": ["sleep", "120"]}
            },
            "volumes": {"state": {}}
        })
        .to_string(),
    )
    .unwrap();
    let base = ["compose", "--project-name", &project.name, "--file", "compose.json"];
    let services = project.live(&[&base[..], &["logs"]].concat(), "");
    assert!(
        services.contains("worker\tCompose service") && services.contains("debugger\tCompose service"),
        "{services:?}"
    );
    assert!(project.live(&base, "--profile=deb").contains("--profile=debug"));
    assert!(project.live(&[&base[..], &["up"]].concat(), "--scale=wor").contains("--scale=worker=\t"));
    assert!(project.live(&[&base[..], &["cp"]].concat(), "wor").contains("worker:\t"));
    checked(project.compose(&["up", "--detach", "worker"]).output().unwrap());
    let containers = project.live(&["exec"], &project.name);
    assert!(
        containers.contains("worker") && containers.contains(&image),
        "{containers:?}"
    );
    assert!(project.live(&["run"], &image).contains(&image));
    let target = containers.lines().find_map(|row| row.split_once('\t').map(|(name, _)| name)).unwrap();
    assert!(project.live(&["update", target], "--restart=").contains("--restart=on-failure:3\tCurrent restart policy"));
    let mount = project.live(&["run"], &format!("--mount=type=volume,source={}", project.name));
    assert!(mount.contains(&format!("{}_state\t", project.name)), "{mount:?}");
    let filter = project.live(&["ps"], &format!("--filter=volume={}", project.name));
    assert!(filter.contains(&format!("{}_state\t", project.name)), "{filter:?}");
    checked(project.compose(&["stop", "--timeout", "1", "worker"]).output().unwrap());
    assert!(project.live(&["exec"], &project.name).is_empty());
    let logs = project.live(&["logs"], &project.name);
    assert!(logs.contains("worker") && logs.contains("Exited"), "{logs:?}");
}

#[test]
#[cfg(target_os = "linux")]
fn podman_reads_disposable_pods_volumes_networks_and_secret_names() {
    if std::env::var_os("COMPLETE_ME_TEST_PODMAN").is_none() {
        eprintln!("opt in with COMPLETE_ME_TEST_PODMAN=1 to test rootless Podman");
        return;
    }
    let dir = Scratch::new(&std::env::temp_dir(), "podman");
    let data = dir.path().join("store");
    let runtime = dir.path().join("run");
    let config = dir.path().join("containers.conf");
    let payload = dir.path().join("fixture.secret");
    std::fs::write(&config, "[engine]\ncgroup_manager = 'cgroupfs'\nevents_logger = 'file'\n").unwrap();
    std::fs::write(&payload, "fixture secret body").unwrap();
    let scope = ["--root", data.to_str().unwrap(), "--runroot", runtime.to_str().unwrap()];
    let env = |program: &str| {
        let mut cmd = Command::new(program);
        cmd.current_dir(dir.path()).env("STORAGE_DRIVER", "vfs").env("CONTAINERS_CONF", &config)
            .env_remove("CONTAINER_HOST").env_remove("CONTAINER_CONNECTION").env_remove("COMPLETE_ME_NO_REMOTE");
        cmd
    };
    let podman = |args: &[&str]| checked(env("podman").args(scope).args(args).output().unwrap());
    let live = |args, current| checked(env(env!("CARGO_BIN_EXE_complete-me"))
        .args(["live", "--current", current, "--", "podman"]).args(scope)
        .args(args).output().unwrap());
    podman(&["pod", "create", "--infra=false", "--name", "cm-fixture-pod"]);
    podman(&["volume", "create", "cm-fixture-volume"]);
    podman(&["network", "create", "cm-fixture-network"]);
    podman(&["secret", "create", "cm-fixture-key", payload.to_str().unwrap()]);
    for (args, current, expected) in [
        (&["run"][..], "--pod=cm-", "--pod=cm-fixture-pod\tPodman pod"),
        (&["pod", "ps"][..], "--filter=name=cm-", "--filter=name=cm-fixture-pod\tPodman pod"),
        (&["run"][..], "--secret=cm-", "--secret=cm-fixture-key\tPodman secret"),
        (&["run"][..], "--mount=type=volume,source=cm-", "source=cm-fixture-volume\t"),
        (&["network", "inspect"][..], "cm-", "cm-fixture-network\t"),
    ] {
        let rows = live(args, current);
        assert!(rows.contains(expected), "{args:?}: {rows:?}");
        assert!(!rows.contains("fixture secret body"));
    }
    podman(&["secret", "rm", "cm-fixture-key"]);
    assert!(live(&["run"][..], "--secret=cm-").is_empty());
    podman(&["pod", "rm", "cm-fixture-pod"]);
    podman(&["volume", "rm", "cm-fixture-volume"]);
    podman(&["network", "rm", "cm-fixture-network"]);
}

#[test]
#[cfg(target_os = "linux")]
fn linux_help_probe_uses_read_only_files_and_private_namespaces() {
    if std::env::var_os("COMPLETE_ME_TEST_SANDBOX").is_none() {
        eprintln!("opt in with COMPLETE_ME_TEST_SANDBOX=1 to test bubblewrap");
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let project = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = Scratch::new(project, "sandbox");
    let temporary = Scratch::new(Path::new("/tmp"), "sandbox-temp");
    let original = workspace.path().join("fixture.txt");
    let shadow = temporary.path().join("fixture.txt");
    std::fs::write(&original, "original").unwrap();
    std::fs::write(&shadow, "hidden by sandbox").unwrap();
    let host_namespace = std::fs::read_link("/proc/self/ns/net").unwrap();
    let probe = workspace.path().join("help-fixture");
    let q = |path: &Path| complete_me::util::sq(path.to_str().unwrap());
    std::fs::write(
        &probe,
        format!(
            r#"#!/bin/sh
[ "$#" -eq 1 ] && [ "$1" = "--help" ] || exit 2
printf '%s\n' 'usage: help-fixture'
[ ! -e {shadow} ] && printf '%s\n' 'temp-private'
if (printf changed > {original}) 2>/dev/null; then printf '%s\n' 'file-writable'; fi
printf 'network=%s\n' "$(readlink /proc/self/ns/net)"
"#,
            original = q(&original),
            shadow = q(&shadow)
        ),
    )
    .unwrap();
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        complete_me::probe::sandbox_prefix().is_some(),
        "bubblewrap must be available"
    );
    let start = Instant::now();
    let output = complete_me::probe::help_text(&[probe.to_string_lossy().into_owned()], 4.0);
    assert!(
        output.contains("usage: help-fixture") && output.contains("temp-private"),
        "{output:?}"
    );
    assert!(
        output
            .lines()
            .any(|line| line.starts_with("network=net:[") && !line.ends_with(host_namespace.to_str().unwrap())),
        "{output:?}"
    );
    assert!(!output.contains("file-writable"));
    assert_eq!(std::fs::read_to_string(&original).unwrap(), "original");
    assert!(start.elapsed() < Duration::from_secs(8));
}

#[test]
fn kubernetes_routes_duplicate_names_in_a_disposable_cluster() {
    let Some(context) = std::env::var_os("COMPLETE_ME_TEST_KUBE_CONTEXT") else {
        eprintln!("set COMPLETE_ME_TEST_KUBE_CONTEXT to opt into the disposable Kubernetes check");
        return;
    };
    let context = context.to_str().expect("literal context name");
    let query = |args: &[&str]| {
        checked(Command::new(env!("CARGO_BIN_EXE_complete-me"))
            .env_remove("COMPLETE_ME_NO_REMOTE").args(args).output().unwrap())
    };
    let candidates = query(&["live", "--current", "", "--", "kube", context, "@all", "logs"]);
    for ns in ["cm-left", "cm-right"] {
        assert!(candidates.contains(&format!("{ns}/worker\tpods")), "{candidates:?}");
        let container = query(&["live", "--current", "", "--", "kube", context, "@all", "logs",
            &format!("{ns}/worker"), "-c"]);
        assert!(container.contains("worker\tcontainer · registry.k8s.io/pause:3.10"), "{container:?}");
        let result = query(&["kube", context, "@all", "get", "pod", &format!("{ns}/worker")]);
        assert!(result.lines().any(|line| line.starts_with("worker ")), "{result:?}");
    }
    let description = query(&["kube", context, "@all", "describe", "pod", "cm-right/worker"]);
    assert!(description.lines().any(|line| line.strip_prefix("Namespace:").is_some_and(|ns| ns.trim() == "cm-right")));
}
