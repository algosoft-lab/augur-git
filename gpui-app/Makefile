.PHONY: default dev format build \
	build-bins build-bins-no-ai \
	build-windows build-windows-no-ai \
	build-mac build-mac-no-ai \
	build-appimage build-appimage-no-ai

default: dev

dev:
	cargo run --bin augur-git

format:
	cargo fmt && stylua . && mint fmt .

# `make build` packages the host platform in both variants; the scripts build
# the executables themselves. Use the explicit targets below for one variant.
ifeq ($(OS),Windows_NT)
HOST_PLATFORM := windows
else
UNAME_S := $(shell uname -s)
ifeq ($(UNAME_S),Darwin)
HOST_PLATFORM := mac
else
HOST_PLATFORM := appimage
endif
endif

build: build-$(HOST_PLATFORM) build-$(HOST_PLATFORM)-no-ai

# Cargo-only builds without packaging.
build-bins:
	cargo build --release --bins

build-bins-no-ai:
	cargo build --release --bins --no-default-features

build-windows:
	uv run packaging/build-windows.py

build-windows-no-ai:
	uv run packaging/build-windows.py --no-default-features

build-mac:
	uv run packaging/build-mac-app.py

build-mac-no-ai:
	uv run packaging/build-mac-app.py --no-default-features

build-appimage:
	uv run packaging/build-appimage.py

build-appimage-no-ai:
	uv run packaging/build-appimage.py --no-default-features
