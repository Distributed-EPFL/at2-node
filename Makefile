FEATURE=--features server,client

test:
	cargo test $(FEATURE)

example:
	cargo build --example double_spend $(FEATURE)

run_example:
	RUST_BACKTRACE=1 cargo run --example double_spend $(FEATURE)
