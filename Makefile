.PHONY: fmt check test clippy build
fmt:
	cargo fmt --all -- --check
check:
	cargo check

test:
	cargo test
clippy:
	cargo clippy --all-targets --all-features -- -D warnings
build:
	cargo build --release
