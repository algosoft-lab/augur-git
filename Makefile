.PHONY: dev run format build-noai

default: dev

dev:
	cargo run --bin augur-git

build:
	cargo build --release --bins

build-noai:
	cargo build --release --bins --no-default-features

format:
	cargo fmt && stylua . && mint fmt .

build-win:
	cargo build --release && uv run ./packaging/build-windows.py

build-mac:
	cargo build --release && uv run ./packaging/build-mac-app.py

build-appimage:
	cargo build --release && uv run ./packaging/build-appimage.py
