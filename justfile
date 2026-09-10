set dotenv-load := true

dev:
    npm --prefix web run dev

demo:
    cargo run -p rf-server -- --device mock

test-rust:
    cargo test --workspace

test-web:
    npm --prefix web test

test: test-rust test-web

check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    npm --prefix web run check
    npm --prefix web run build

desktop:
    npm --prefix apps/desktop run tauri dev
