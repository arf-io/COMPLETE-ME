"""The structured model every parser fills in and every shell renderer reads."""
from __future__ import annotations

from dataclasses import dataclass, field, asdict


@dataclass
class Option:
    shorts: list[str] = field(default_factory=list)   # "a"   for -a
    longs: list[str] = field(default_factory=list)    # "all" for --all
    olds: list[str] = field(default_factory=list)     # "name" for find-style -name
    arg: str | None = None                            # placeholder, e.g. "WHEN"
    arg_optional: bool = False                        # --color[=WHEN]
    desc: str = ""
    values: list[tuple[str, str]] = field(default_factory=list)  # (value, description)
    kind: str = ""                                    # file|dir|user|host|pid|command|number|""

    @property
    def key(self) -> str:
        if self.longs:
            return "--" + self.longs[0]
        if self.olds:
            return "-" + self.olds[0]
        return "-" + self.shorts[0]

    def flags(self) -> list[str]:
        return ["-" + s for s in self.shorts] + ["--" + l for l in self.longs] + ["-" + o for o in self.olds]


@dataclass
class Command:
    name: str
    desc: str = ""
    aliases: list[str] = field(default_factory=list)
    options: list[Option] = field(default_factory=list)
    subcommands: dict[str, "Command"] = field(default_factory=dict)
    takes_files: bool = True
    sources: list[str] = field(default_factory=list)
    subcommands_locked: bool = False

    def option(self, flag: str) -> Option | None:
        for o in self.options:
            if flag in o.flags():
                return o
        return None

    def merge(self, other: "Command") -> None:
        """Fold another parse of the same command into this one; existing data wins."""
        if not self.desc:
            self.desc = other.desc
        for o in other.options:
            mine = next((m for m in self.options if set(m.flags()) & set(o.flags())), None)
            if mine is None:
                self.options.append(o)
                continue
            for attr in ("shorts", "longs", "olds"):
                for v in getattr(o, attr):
                    if v not in getattr(mine, attr):
                        getattr(mine, attr).append(v)
            mine.desc = mine.desc or o.desc
            mine.arg = mine.arg or o.arg
            mine.values = mine.values or o.values
            mine.kind = mine.kind or o.kind
        for name, sub in other.subcommands.items():
            if name in self.subcommands:
                self.subcommands[name].merge(sub)
            elif not any(name in s.aliases for s in self.subcommands.values()):
                self.subcommands[name] = sub
        self.takes_files = self.takes_files and other.takes_files
        for s in other.sources:
            if s not in self.sources:
                self.sources.append(s)

    def walk(self, path: tuple[str, ...] = ()):
        yield path, self
        for name, sub in self.subcommands.items():
            yield from sub.walk(path + (name,))

    def to_dict(self) -> dict:
        return asdict(self)
