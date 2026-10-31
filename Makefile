# COMPLETE-ME
.PHONY: install install-yes uninstall test build check

install:        ## interactive install: deps, shell hooks, self-test
	./install.sh

install-yes:    ## non-interactive install, yes to everything
	./install.sh --yes

uninstall:
	./install.sh --uninstall

test:
	cargo test --release

build:
	cargo build --release

check:          ## syntax-check the shell parts
	bash -n install.sh shell/bash/complete_me.bash
	fish -n shell/fish/complete_me.fish
