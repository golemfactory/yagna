build:
	cargo build --release --manifest-path=outbound-bench/Cargo.toml
	docker build -t outbound-test-v5 .
	gvmkit-build outbound-test-v5

push: build
	gvmkit-build outbound-test-v5 --push