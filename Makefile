.PHONY: dev run format

default: dev

dev:
	cargo run

run:
	cargo run --release

format:
	cargo fmt && stylua .
