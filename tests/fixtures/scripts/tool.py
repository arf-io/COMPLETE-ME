#!/usr/bin/env python3
import argparse
p = argparse.ArgumentParser()
p.add_argument("--mode", choices=["fast", "slow"], help="How fast. Really.")
p.add_argument("-v", "--verbose", action="store_true")
p.add_argument("-o", "--output", metavar="FILE", help="Write here")
sp = p.add_subparsers()
run = sp.add_parser("run", help="Run it")
run.add_argument("--dry-run", action="store_true")
sp.add_parser("stop", help="Stop it")
