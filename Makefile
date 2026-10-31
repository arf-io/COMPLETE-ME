# COMPLETE-ME
.PHONY: install install-yes uninstall test check

install:        ## interactive install: deps, shell hooks, self-test
	./install.sh

install-yes:    ## non-interactive install, yes to everything
	./install.sh --yes

uninstall:
	./install.sh --uninstall

test:
	python3 -m unittest discover -s tests -v

check:          ## syntax-check the shell parts
	bash -n install.sh shell/bash/complete_me.bash
	fish -n shell/fish/complete_me.fish
