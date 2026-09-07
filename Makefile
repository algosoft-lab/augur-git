.PHONY: dev run format

default: dev

dev:
	cargo run --bin augur-git

build:
	cargo build --release --bins

format:
	cargo fmt && stylua .

build-win:
	cargo build --release && uv run ./packaging/build-windows.py

build-mac:
	cargo build --release && uv run ./packaging/build-mac-app.py

build-appimage:
	cargo build --release && uv run ./packaging/build-appimage.py
