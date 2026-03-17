.PHONY: build universal clean

build:
	cargo build --release

universal:
	rustup target add x86_64-apple-darwin aarch64-apple-darwin
	cargo build --release --target x86_64-apple-darwin
	cargo build --release --target aarch64-apple-darwin
	lipo -create \
		target/x86_64-apple-darwin/release/audioleaf \
		target/aarch64-apple-darwin/release/audioleaf \
		-output target/release/audioleaf-universal

clean:
	cargo clean
