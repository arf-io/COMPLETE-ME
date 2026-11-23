# COMPLETE-ME launch outreach

Use the public GitHub repository as the link people can open immediately. Once the site is
public, include the demo as a second link. Lead with a command the recipient can recognize.
For developer communities, invite one piece of feedback you can use, such as a completion
slot that selects the wrong value or a shell binding that conflicts with their setup.

## Start here

| Where | What to share | How |
|---|---|---|
| GitHub | Put the install command, a terminal screenshot, and the six example workflows on the project’s front door. | Share the [public repository](https://github.com/arf-io/COMPLETE-ME). Use [Releases](https://github.com/arf-io/COMPLETE-ME/releases) when you have selected a release tag. |
| Terminal Trove | A terminal capture of the tmux menu and a second example where a value comes from live state. | Check for an existing listing, then use the [tool submission form](https://terminaltrove.com/submit/) with a public image URL and actual install commands. |
| Hacker News | Explain the Bash, zsh, and fish problem in your own words, with a public repo and a reproducible example. Stay around to answer questions. | Use [Show HN](https://news.ycombinator.com/showhn.html) for a personally built, runnable project. The [HN rules](https://news.ycombinator.com/newsguidelines.html) require human-written submissions and comments. |
| Changelog News | Pitch the cross-shell architecture or an SSH editing demonstration with an installable GitHub link. | Use [Changelog’s news submission route](https://changelog.com/contact). |
| Lobsters | Show how the command grammar and live providers divide responsibility. Use a technical walkthrough with source and concrete edge cases. | Share through an existing account if the piece fits the [guidelines](https://lobste.rs/about). |
| r/commandline | Show the terminal interaction and ask which argument people repeatedly look up by hand. | Read the current [community rules](https://www.reddit.com/r/commandline/about/rules/) and any posting eligibility notice first. |

## Follow with focused posts

| Where | What to share | How |
|---|---|---|
| r/homelab | A Proxmox/PBS walkthrough with a sample VM, selected node, and archive ID. Explain the separation between looking up and submitting a restore. | Follow the moderators’ [software showcase rules and disclosure requirements](https://www.reddit.com/r/homelab/comments/1ty58af/announcement_new_rules_processes_on_software/). |
| Proxmox Community Forum | Share a concrete cluster configuration and ask for feedback from users of PBS and mounted directory storage. | Check the [Proxmox forum](https://forum.proxmox.com/) for the right discussion area or a relevant thread. |
| Docker Community Forums | Explain the value of Compose service names and context-specific container metadata, with a minimal demo. | Look for a relevant thread in [Docker General Discussions](https://forums.docker.com/c/general/18) and follow the forum’s guidance. |
| Fish shell community | Ask for feedback on Alt+J discoverability, mid-line editing, and native Tab integration. Include your fish version and test steps. | Use the support and community links in the [fish documentation](https://fishshell.com/docs/current/#where-to-get-help). |
| r/rust and This Week in Rust | Write a maintainer-authored technical walkthrough of command grammars, subprocess boundaries, or shell test fixtures. | Check the [r/rust rules](https://www.reddit.com/r/rust/about/rules/). Editors now consider project/tooling links from r/rust rather than project-update PRs. See the [editor announcement](https://github.com/rust-lang/this-week-in-rust/issues/8575). |
| Platform engineering and homelab meetups | Bring a five-minute demo. Use tmux first, then the same completion mechanic for a Proxmox archive or S3 key. | Contact the organizer of a group you participate in. Offer the runnable repository and a short demo recording. |
| Your own blog and social accounts | Post a short recording and one explanation of the user problem. Put the public repo on the first line after the caption. | Start with the channels you already maintain. Use alt text or captions for the terminal recording. |

For a technical write-up, show the new Python command being written in the terminal and explain
how its options become available on the first Tab.

## Assets to include

- [Press release](press-release.md) for editors and project announcements.
- Capture a frame of the side-by-side Python options example after Tab. The
  [demo setup](../site/README.md) shows how to run the real terminal locally.
- [Completion reference](completion.md) and [verification](verification.md) for technical coverage.
- For a short recording, show tmux, a cursor inside the SSH identity-file argument, and the
  Proxmox or S3 example. Keep hostnames, identities, and resource names synthetic.

## Short editor pitch

**Subject** COMPLETE-ME adds live argument completion across Bash, zsh, and fish

COMPLETE-ME is an MIT-licensed Rust CLI that generates missing shell completions and supplies
live values when it knows the argument. Try a tmux window, a Docker container with its image and
status, or an S3 key. It also completes inside an existing command, so an SSH identity-file path
can fill in without disturbing the host to its right. The public repository has the installer,
examples, and verification notes.

[Project and installation](https://github.com/arf-io/COMPLETE-ME)
