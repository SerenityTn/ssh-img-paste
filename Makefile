SHELL := /bin/bash

.PHONY: build check test lint icon screenshot install uninstall

build:
	./build.sh

check: lint test

test:
	./tests/run-all.sh

lint:
	@command -v shellcheck >/dev/null || { echo "shellcheck is required: brew install shellcheck" >&2; exit 1; }
	/bin/bash -n bin/ssh-img-paste build.sh install.sh uninstall.sh scripts/*.sh tests/*.sh
	shellcheck -S error bin/ssh-img-paste build.sh install.sh uninstall.sh scripts/*.sh tests/*.sh

icon:
	./scripts/generate-app-icon.sh

screenshot:
	./scripts/capture-profile-manager-screenshot.sh

install:
	./install.sh

uninstall:
	./uninstall.sh
