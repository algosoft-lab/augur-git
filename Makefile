.PHONY: dev run format

default: dev

dev:
	cargo run

run:
	cargo run --release

format:
	cargo fmt && stylua .

build-win:
	cargo build --release && uv run ./packaging/build-windows.py

build-mac:
	cargo build --release && uv run ./packaging/build-mac-app.py

build-appimage:
	cargo build --release && uv run ./packaging/build-appimage.py
