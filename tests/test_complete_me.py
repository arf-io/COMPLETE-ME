"""Run with: python3 -m unittest discover -s tests -v"""
import os
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from complete_me import bashfish, findhelp, regex, render, scriptscan, sshopts  # noqa: E402
from complete_me.model import Command  # noqa: E402
from complete_me.parse import parse_help  # noqa: E402

FIX = ROOT / "tests" / "fixtures"


def fixture(name: str) -> Command:
    return parse_help((FIX / f"{name}.help").read_text(), name)


class ParseHelp(unittest.TestCase):
    def test_git_style_grouped_commands(self):
        c = fixture("git")
        for sub in ("clone", "commit", "push", "rebase"):
            self.assertIn(sub, c.subcommands)
        self.assertEqual(c.subcommands["clone"].desc, "Clone a repository into a new directory")
        for junk in ("cluding", "ken", "mit", "the"):           # the old analyzer's man-page word salad
            self.assertNotIn(junk, c.subcommands)
        self.assertIsNotNone(c.option("-C"))                     # harvested from the usage block
        self.assertEqual(c.option("-C").kind, "file")

    def test_gnu_short_and_long_with_commas(self):
        c = fixture("ls")
        for flag in ("-a", "-A", "-d", "-h", "-r", "-R", "--all", "--human-readable"):
            self.assertIsNotNone(c.option(flag), flag)
        self.assertEqual([v for v, _ in c.option("--color").values], ["always", "auto", "never"])
        self.assertIn("extension", [v for v, _ in c.option("--sort").values])

    def test_clap_possible_values_and_aliases(self):
        c = fixture("cargo")
        self.assertEqual([v for v, _ in c.option("--color").values], ["auto", "always", "never"])
        self.assertEqual(c.subcommands["build"].aliases, ["b"])
        self.assertNotIn("...", c.subcommands)
        self.assertIsNotNone(c.option("--list"))                 # deeper-indented long-only option
        self.assertEqual(c.option("-C").kind, "dir")

    def test_rg_long_help_value_descriptions(self):
        c = fixture("rg")
        vals = dict(c.option("--color").values)
        self.assertIn("never", vals)
        self.assertTrue(vals["never"])                           # value has its own description
        self.assertIn("pcre2", [v for v, _ in c.option("--engine").values])

    def test_subcommand_only_cli(self):
        c = fixture("zerotier-cli")
        self.assertEqual(list(c.subcommands)[:4], ["info", "listpeers", "peers", "listnetworks"])
        self.assertIsNone(c.option("-s"))                        # "[-switches]" is not a flag
        self.assertEqual(c.option("-p").kind, "number")

    def test_cobra_and_docker_sections(self):
        c = fixture("docker")
        self.assertIn("compose", c.subcommands)                  # "compose*" plugin marker stripped
        self.assertEqual([v for v, _ in c.option("--log-level").values], ["debug", "info", "warn", "error", "fatal"])

    def test_commander_js(self):
        c = fixture("claude-mcp")
        self.assertIn("add-json", c.subcommands)
        self.assertNotIn("Examples:", c.subcommands)

    def test_headerless_glib_table(self):
        text = textwrap.dedent("""\
            Usage:
              tool [OPTION…]

              activate [DEVICE-ID|GUID]         Activate pending devices
              crc KIND FILENAME                 Calculates a CRC of a file
              clear-history                     Erase all firmware update history

            Help Options:
              -h, --help                        Show help options
            """)
        c = parse_help(text, "tool")
        self.assertEqual(list(c.subcommands), ["activate", "crc", "clear-history"])
        self.assertTrue(c.subcommands["crc"].takes_files)
        self.assertFalse(c.subcommands["activate"].takes_files)


class ScriptScan(unittest.TestCase):
    def test_bash_dispatcher_and_usage(self):
        with tempfile.NamedTemporaryFile("w", suffix=".sh", delete=False) as f:
            f.write(textwrap.dedent("""\
                #!/bin/bash
                usage() {
                  cat <<'USAGE'
                tool — does things
                  tool push <bundle.tar>   upload a bundle
                  tool status              show what is live
                USAGE
                }
                while getopts "vf:" opt; do :; done
                case "$1" in
                  push) shift ;;
                  status) ;;
                  rollback) ;;   # go back a release
                  -q|--quiet) ;;
                  *) usage ;;
                esac
                """))
        c = scriptscan.scan(f.name, "tool")
        os.unlink(f.name)
        self.assertEqual(c.subcommands["push"].desc, "upload a bundle")
        self.assertTrue(c.subcommands["push"].takes_files)
        self.assertEqual(c.subcommands["rollback"].desc, "go back a release")
        for flag in ("-v", "-f", "-q", "--quiet"):
            self.assertIsNotNone(c.option(flag), flag)
        self.assertEqual(c.option("-f").arg, "ARG")

    def test_python_argparse(self):
        with tempfile.NamedTemporaryFile("w", suffix=".py", delete=False) as f:
            f.write(textwrap.dedent("""\
                #!/usr/bin/env python3
                import argparse
                p = argparse.ArgumentParser()
                p.add_argument("--mode", choices=["fast", "slow"], help="How fast. Really.")
                p.add_argument("-v", "--verbose", action="store_true")
                sp = p.add_subparsers()
                run = sp.add_parser("run", help="Run it")
                run.add_argument("--dry-run", action="store_true")
                """))
        c = scriptscan.scan(f.name, "t")
        os.unlink(f.name)
        self.assertEqual(c.option("--mode").values, [("fast", ""), ("slow", "")])
        self.assertIsNone(c.option("-v").arg)
        self.assertIsNotNone(c.subcommands["run"].option("--dry-run"))
        self.assertIsNone(c.option("--dry-run"))


class Render(unittest.TestCase):
    def test_hostile_help_text_is_quoted(self):
        c = Command(name="evil", sources=["help"])
        c.subcommands["ok"] = Command(name="ok", desc="it's $(rm -rf ~) `boom` \\ done")
        c.subcommands["bad;rm"] = Command(name="bad;rm")
        out = render.fish(c)
        self.assertNotIn("bad;rm", out)
        self.assertIn("'it\\'s $(rm -rf ~) `boom` \\\\ done'", out)
        with self.assertRaises(ValueError):
            render.fish(Command(name="../../x"))

    def test_bash_renders(self):
        out = render.bash(fixture("cargo"))
        self.assertIn("complete -o bashdefault -o default -F _complete_me_cargo cargo", out)


class Regex(unittest.TestCase):
    def rows(self, p, f):
        return [(t, w) for _, t, _, w in regex.explain(p, f)]

    def test_flavour_detection(self):
        self.assertEqual(regex.detect(["grep", "-rnE", "x"]), ("grep", "ere"))
        self.assertEqual(regex.detect(["grep", "x"]), ("grep", "bre"))
        self.assertEqual(regex.detect(["rg", "x"]), ("rg", "rust"))
        self.assertEqual(regex.detect(["rg", "--pcre2"]), ("rg", "pcre"))
        self.assertEqual(regex.detect(["grep", "-F"]), ("grep", "fixed"))
        self.assertEqual(regex.detect(["find", ".", "-regextype", "posix-extended"]), ("find", "ere"))

    def test_digit_class_warns_outside_pcre(self):
        self.assertEqual(self.rows(r"\d", "pcre"), [("\\d", "")])
        self.assertIn("not supported", self.rows(r"\d", "ere")[0][1])

    def test_bre_escaping(self):
        rows = self.rows(r"a\(b\)\{2\}c+", "bre")
        toks = [t for t, _ in rows]
        self.assertEqual(toks, ["a", "\\(", "b", "\\)", "{2}", "c", "+"])
        self.assertIn("literal", rows[-1][1])

    def test_literals_are_not_lost(self):
        toks = [t for t, _ in self.rows(r"[^ ]+@x\.com", "ere")]
        self.assertEqual(toks, ["[^ ]", "+", "@x", "\\.", "com"])

    def test_lookaround_needs_pcre_in_rg(self):
        self.assertIn("rg -P", self.rows(r"(?<=a)b", "rust")[0][1])
        self.assertEqual(self.rows(r"(?<=a)b", "pcre")[0][1], "")

    def test_recipes_compile_in_their_flavour(self):
        import re
        for _, ere, modern in regex.RECIPES:
            re.compile(modern.replace("[[:", "[").replace(":]]", "]"))
            if shutil.which("grep"):
                for flav, mode in (("ere", "-E"), ("bre", "-G")):
                    pat = regex.recipe_for(flav, ere, modern)
                    r = subprocess.run(["grep", mode, "-q", "-e", pat], input="", text=True, capture_output=True)
                    self.assertIn(r.returncode, (0, 1), f"{flav}: {pat}: {r.stderr}")

    def test_trailing_backslash_offers_escapes(self):
        c = regex.candidates(["grep", "-E"], "'ab\\")
        self.assertTrue(c)
        self.assertTrue(all(v.startswith("ab\\") for v, _ in c))


class Find(unittest.TestCase):
    def test_or_action_trap(self):
        out = findhelp.explain("find . -name a -o -name b -exec rm {} ;".split())
        self.assertIn("only runs for that side", out)

    def test_delete_first(self):
        self.assertIn("Put -delete last", findhelp.explain("find /x -delete -name a".split()))

    def test_batched_exec(self):
        out = findhelp.explain("find . -type f -exec rm {} +".split())
        self.assertIn("run `rm FILE` once with MANY files", out)
        self.assertNotIn("⚠", out)

    def test_values(self):
        self.assertIn(("d", "directory"), findhelp.values_for("-type"))
        self.assertTrue(findhelp.values_for("-size"))


class SshConfig(unittest.TestCase):
    MAN = "\n".join([
        "     PasswordAuthentication",
        "             Specifies whether to use password authentication.  The argument to this keyword must",
        "             be yes (the default) or no.",
        "",
        "     LogLevel",
        "             Gives the verbosity level.  The possible values are: QUIET, FATAL, ERROR, and DEBUG3.",
        "",
        "     StrictHostKeyChecking",
        "             If this flag is set to yes, ssh will never add host keys.  If this flag is set to",
        "             accept-new then ssh will add new keys.  If this flag is set to no or off, ssh will.",
    ])

    def test_values(self):
        k = sshopts.parse_ssh_config_man(self.MAN)
        self.assertEqual(k["PasswordAuthentication"]["values"], ["yes", "no"])
        self.assertEqual(k["LogLevel"]["values"], ["QUIET", "FATAL", "ERROR", "DEBUG3"])
        self.assertEqual(k["StrictHostKeyChecking"]["values"], ["yes", "accept-new", "no", "off"])


class BashToFish(unittest.TestCase):
    def test_risks(self):
        self.assertIn("downloads a script and runs it", bashfish.risks("curl -fsSL https://x | bash"))
        self.assertIn("rm -r/-f: deletes without asking", bashfish.risks('for f in *; do rm -rf "$f"; done'))
        self.assertIn("runs as root", bashfish.risks("sudo systemctl restart x"))
        self.assertEqual(bashfish.risks('for i in 1 2; do echo "$i"; done'), [])
        self.assertEqual(bashfish.risks("echo sudoku; echo formula"), [])

    def test_brace_ranges(self):
        self.assertEqual(bashfish.preprocess("for i in {1..3}; do :; done"), "for i in $(seq 1 3); do :; done")
        self.assertEqual(bashfish.preprocess("echo {5..1}"), "echo $(seq 5 -1 1)")
        self.assertEqual(bashfish.preprocess("echo '{1..3}'"), "echo '{1..3}'")

    @unittest.skipUnless(shutil.which("babelfish") and shutil.which("fish"), "babelfish/fish not installed")
    def test_translation_runs_in_fish(self):
        code, err = bashfish.translate('for i in {1..3}; do echo "n=$i"; done; x=$(echo hi); echo ${x:-none}')
        self.assertEqual(err, "")
        out = subprocess.run(["fish", "--no-config", "-c", code], capture_output=True, text=True, timeout=10)
        self.assertEqual(out.stdout.split(), ["n=1", "n=2", "n=3", "hi"])


@unittest.skipUnless(shutil.which("fish"), "fish not installed")
class FishEndToEnd(unittest.TestCase):
    """Ask fish itself what it would complete, with complete-me loaded in an isolated environment."""

    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.mkdtemp(prefix="complete-me-test-")
        cls.env = dict(os.environ, XDG_CACHE_HOME=cls.tmp + "/cache", XDG_CONFIG_HOME=cls.tmp + "/config",
                       XDG_DATA_HOME=cls.tmp + "/data", complete_me_force="1",
                       PATH=f"{ROOT / 'bin'}:{os.environ['PATH']}")
        gen = Path(cls.tmp) / "cache" / "complete-me" / "fish"
        gen.mkdir(parents=True)
        (gen / "zerotier-cli.fish").write_text(render.fish(fixture("zerotier-cli")))
        (gen / "cargo-like.fish").write_text(render.fish(fixture("cargo")).replace("-c cargo ", "-c cargo-like "))
        hist = Path(cls.tmp) / "data" / "fish"
        hist.mkdir(parents=True)
        (hist / "fish_history").write_text("- cmd: zerotier-cli join 8056c2e21c000001\n  when: 1790000000\n")

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.tmp, ignore_errors=True)

    def complete(self, line: str) -> list[str]:
        script = (f"set -g fish_history fish; source {ROOT / 'shell/fish/complete_me.fish'}; "
                  f"function cargo-like; end; complete -C {render.fq(line)}")
        out = subprocess.run(["fish", "--no-config", "-c", script], env=self.env, capture_output=True,
                             text=True, timeout=30, cwd=self.tmp)
        self.assertEqual(out.stderr, "")
        return [l.split("\t")[0] for l in out.stdout.splitlines()]

    def test_subcommands_then_their_args(self):
        self.assertIn("listnetworks", self.complete("zerotier-cli list"))
        self.assertIn("8056c2e21c000001", self.complete("zerotier-cli join "))   # from history

    def test_alias_path_and_enum_values(self):
        self.assertEqual(self.complete("cargo-like --color "), ["always", "auto", "never"])
        self.assertIn("--color", self.complete("cargo-like --co"))

    def test_find_exec_terminators(self):
        self.assertEqual(self.complete("find . -exec rm ")[0], "{}")
        self.assertEqual(self.complete("find . -exec rm {} ")[:2], [";", "+"])

    def test_regex_slot(self):
        got = self.complete("grep -E ab")
        self.assertIn("ab[0-9]", got)
        self.assertNotIn("ab[0-9]", self.complete("grep -E pat ab"))   # second positional is a file

    def test_clipboard_is_offered_only_when_it_fits(self):
        def with_clip(text, line):
            env = dict(self.env, COMPLETE_ME_CLIPBOARD=text)
            script = (f"source {ROOT / 'shell/fish/complete_me.fish'}; complete -C {render.fq(line)}")
            out = subprocess.run(["fish", "--no-config", "-c", script], env=env, capture_output=True, text=True,
                                 timeout=30, cwd=self.tmp)
            return [l.split("\t")[0] for l in out.stdout.splitlines() if "clipboard" in l]
        self.assertEqual(with_clip("https://github.com/x/y", "git clone "), ["https://github.com/x/y"])
        self.assertEqual(with_clip("git@host:me/repo.git", "git clone "), ["git@host:me/repo.git"])
        self.assertEqual(with_clip("https://example.com/f.tgz", "git clone "), [])      # not a git URL
        self.assertEqual(with_clip("https://example.com/f.tgz", "curl -O "), ["https://example.com/f.tgz"])
        self.assertEqual(with_clip("hunter2 secret words", "curl "), [])                # never leak plain text
        self.assertEqual(with_clip("https://a.example\nhttps://b.example", "curl "), [])  # one line only
        self.assertEqual(with_clip(self.tmp + "/data", "cd "), [self.tmp + "/data/"])
        self.assertEqual(with_clip(self.tmp + "/nope", "less "), [])                    # must exist

    def test_ssh_option_values(self):
        if not shutil.which("man") or not sshopts.man_text("ssh_config"):
            self.skipTest("no ssh_config man page")
        got = self.complete("ssh -oPasswordAuthentication=")
        self.assertEqual(sorted(got), ["-oPasswordAuthentication=no", "-oPasswordAuthentication=yes"])


if __name__ == "__main__":
    unittest.main()
