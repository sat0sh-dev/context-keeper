# Contributing to ContextKeeper

Thank you for your interest in contributing!

## How to Contribute

### Bug Reports & Feature Requests

Please open an issue on GitHub with:
- Clear description of the problem or feature
- Steps to reproduce (for bugs)
- Your environment (OS, Rust version, etc.)

### Pull Requests

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes
4. Run tests: `cargo test`
5. Run clippy: `cargo clippy`
6. Commit your changes
7. Push to your fork and open a Pull Request

### Development Setup

```bash
git clone https://github.com/YOUR_USERNAME/context-keeper
cd context-keeper
cargo build
cargo test
```

### Code Style

- Follow standard Rust conventions
- Run `cargo fmt` before committing
- Ensure `cargo clippy` passes without warnings

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
