.PHONY: dev-ui

# Rebuild and relaunch the native UI whenever its Rust code or bundled assets change.
dev-ui:
	@command -v watchexec >/dev/null 2>&1 || { \
		echo "watchexec is required; install it with: brew install watchexec"; \
		exit 1; \
	}
	watchexec \
		--restart \
		--stop-timeout 2s \
		--watch src \
		--watch assets \
		--watch Cargo.toml \
		--exts rs,toml,ttf \
		-- cargo run -- ui
