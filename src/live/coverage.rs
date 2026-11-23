//! A static inventory so empty runtimes do not look like missing providers.
use serde_json::json;

struct Slot {
    tool: &'static str,
    value: &'static str,
    example: &'static str,
    source: &'static str,
}

macro_rules! catalog {
    ($($tool:literal, $value:literal, $example:literal, $source:literal;)*) => {
        const SLOTS: &[Slot] = &[$(Slot { tool: $tool, value: $value, example: $example, source: $source },)*];
    };
}

catalog! {
    "git", "Config keys", "git config get user.", "Selected Git config and installed Git key catalog";
    "git", "Current config value", "git config set --global user.email ", "Selected Git config, for permitted preferences";
    "git", "Revisions and revision ranges", "git log trunk..fea", "Local heads, tags and remote-tracking refs";
    "git", "Revision option values", "git restore --source=fea", "Local heads, tags and remote-tracking refs";
    "git", "Paths in a revision", "git show HEAD:src/", "Local Git tree objects";
    "git", "Changed paths", "git add ", "Git index and working directory";
    "git", "Tracked paths", "git restore -- ", "Git index";
    "git", "Unmerged paths", "git restore --merge ", "Git index stages";
    "git", "Configured remotes", "git remote set-url ", "Repository remote names";
    "git", "Remote branches", "git fetch origin ", "Local remote-tracking refs";
    "git", "Worktrees", "git worktree remove ", "Registered worktrees";
    "git", "Stashes", "git stash pop ", "Local stash list";
    "git", "Submodules", "git submodule update -- ", "Paths in the selected .gitmodules";
    "docker", "Contexts", "docker --context=", "Local Docker CLI configuration";
    "docker", "Containers and images", "docker container inspect ", "Selected Docker Engine";
    "docker", "Networks and volumes", "docker inspect --type=network ", "Selected Docker Engine";
    "docker", "Mounted volumes", "docker run --mount=type=volume,source=", "Selected Docker Engine";
    "docker", "Filter values", "docker ps --filter=ancestor=", "Selected Docker Engine";
    "docker", "Current restart policy", "docker update worker --restart=", "One field from the selected container";
    "docker", "Plugins and Swarm objects", "docker service scale ", "Selected Docker Engine or Swarm manager";
    "docker", "Compose profiles", "docker compose --profile=", "Selected Compose project";
    "docker", "Compose services and service options", "docker compose up --scale=", "Selected Compose project";
    "podman", "Connections", "podman --connection=", "Local Podman CLI configuration";
    "podman", "Containers and images", "podman container inspect ", "Selected Podman runtime";
    "podman", "Networks and volumes", "podman network inspect ", "Selected Podman runtime";
    "podman", "Mounted volumes", "podman run --mount=type=volume,source=", "Selected Podman runtime";
    "podman", "Filter values", "podman pod ps --filter=name=", "Selected Podman runtime";
    "podman", "Pods and secret names", "podman run --pod=", "Selected Podman runtime";
    "podman", "Machines", "podman machine ssh ", "Local Podman machine configuration";
    "proxmox", "SSH node aliases", "proxmox ", "Configured complete-me/proxmox-hosts file";
    "proxmox", "Container and VM IDs", "proxmox lab-pve pct enter ", "pct list or qm list on the selected node";
    "proxmox", "Cluster guest IDs and routing", "proxmox lab qm terminal ", "Cluster resource inventory and configured node aliases";
    "proxmox", "Guest snapshots", "pct rollback 100 ", "Snapshot tree on the selected node";
    "proxmox", "Migration targets", "qm migrate 410 ", "Cluster node inventory";
    "proxmox", "Storage and volume IDs", "pvesm path local:", "Storage inventory on the selected node";
    "proxmox", "Backup guests and storage", "proxmox lab --node pve02 vzdump ", "Guests and configured backup storage on the selected node";
    "proxmox", "Restore archives, IDs and storage", "qmrestore vault:", "Backup content and restore storage on the selected node, plus cluster next ID";
    "zfs", "Datasets and snapshots", "zfs rollback tank/app@", "ZFS inventory on the selected node";
    "zfs", "Pools", "zpool status ", "Pool list on the selected node";
    "zfs", "Dataset-relative paths", "complete-me zfs-path tank/app logs/", "Mounted local filesystem";
    "kube", "Context and namespace", "kube prod ", "Kubeconfig and selected Kubernetes API";
    "kube", "Qualified pod and resource names", "kube prod @all logs ", "Selected cluster's namespaced resources";
    "aws", "AWS profile and region", "aws --profile=", "Local AWS CLI configuration and selected account";
    "aws", "EC2, EKS, ECS, Lambda, RDS, and S3 resources", "aws ec2 start-instances --instance-ids ", "Bounded inventory in the selected profile and region";
    "aws", "S3 object keys and common prefixes", "aws s3 cp s3://example-bucket/logs/", "One ListObjectsV2 page for the selected bucket and prefix";
    "cloud", "Vultr instances, regions, domains and clusters", "vultr-cli instance get ", "Selected Vultr CLI account";
    "cloud", "Lambda Cloud instances", "cm-cloud lambda instances ", "Lambda Cloud API";
    "cloud", "DreamHost DNS record names", "cm-cloud dreamhost records ", "DreamHost DNS API";
    "cloud", "Hugging Face repositories", "hf download ", "Local Hugging Face cache";
    "agents", "CLI commands and options", "codex exec ", "Bundled agent grammar and local agents.json";
    "agents", "Local session IDs and configured model names", "codex resume ", "Local session metadata and agents.json";
}

const LIMITS: &[(&str, &str, &str)] = &[
    ("git", "Credential-bearing and command-bearing config values", "protected"),
    ("git", "Refs that exist only on a remote server", "local-tracking-refs"),
    ("docker", "Paths or executables inside containers", "requires-execution-or-filesystem-access"),
    ("docker", "Registry-only image tags and buildx builder status", "requires-network-or-plugin-specific-lookup"),
    ("podman", "Paths or executables inside containers", "requires-execution-or-filesystem-access"),
    ("podman", "Registry-only image tags and custom plugin arguments", "requires-network-or-plugin-specific-lookup"),
    ("proxmox", "Guest IP metadata", "opt-in; one matching guest; configured LXC address or QEMU guest agent"),
    ("proxmox", "Cluster guests on nodes without an SSH mapping", "explicit-node-mapping"),
    ("zfs", "Remote paths within datasets", "local-mount-only"),
    ("kube", "Cross-namespace execution outside qualified resource commands", "explicit-namespace-required"),
    ("aws", "Resources outside the selected AWS profile and region", "explicit-scope"),
    ("agents", "Account-side models and remote agent sessions", "local-catalog-and-indexes"),
];

pub(super) fn run(args: &[String], structured: bool) -> i32 {
    let tool = match args {
        [] => None,
        [tool] if SLOTS.iter().any(|slot| slot.tool == tool) => Some(tool.as_str()),
        _ => { eprintln!("usage: complete-me live --coverage [--json] [git|docker|podman|proxmox|zfs|kube|aws|cloud|agents]"); return 2; }
    };
    let slots = SLOTS.iter().filter(|slot| tool.is_none_or(|tool| slot.tool == tool));
    let limits = LIMITS.iter().filter(|(name, _, _)| tool.is_none_or(|tool| *name == tool));
    if structured {
        let slots: Vec<_> = slots.map(|slot| json!({
            "tool": slot.tool, "value": slot.value, "example": slot.example, "source": slot.source,
        })).collect();
        let limits: Vec<_> = limits.map(|(tool, value, status)| json!({
            "tool": tool, "value": value, "status": status,
        })).collect();
        println!("{}", json!({"slots": slots, "limits": limits}));
    } else {
        for slot in slots {
            println!("{}\t{}\t{}\t{}", slot.tool, slot.value, slot.example, slot.source);
        }
        println!("\nCoverage limits");
        for (tool, value, status) in limits {
            println!("{tool}\t{value}\t{status}");
        }
    }
    0
}
