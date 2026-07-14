# Contributing to CodexPlusPlus

Thank you for your interest in contributing to CodexPlusPlus!

## Development Setup

1. **Clone the repository**
   ```bash
   git clone https://github.com/BigPizzaV3/CodexPlusPlus.git
   cd CodexPlusPlus
   ```

2. **Install Rust toolchain**
   Ensure you have Rust 1.70+ installed:
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   rustc --version  # Should be 1.70+
   ```

3. **Build the project**
   ```bash
   cargo build --release
   ```

4. **Run tests**
   ```bash
   cargo test
   ```

## Project Structure

```
CodexPlusPlus/
├── crates/
│   ├── codex-plus-data/    # Data handling and provider sync
│   └── codex-plus-core/    # Core Codex++ logic
└── README.md               # Project documentation
```

## Making Changes

1. **Create a feature branch**
   ```bash
   git checkout -b feat/your-feature-name
   ```

2. **Make your changes**
   - Write idiomatic Rust code
   - Add tests for new functionality
   - Update documentation as needed

3. **Run the test suite**
   ```bash
   cargo test --all-features
   cargo clippy  # Linting
   ```

4. **Commit your changes**
   ```bash
   git commit -m "feat: add your feature description"
   ```

## Code Style

- Follow Rust standard formatting (`cargo fmt`)
- Use `clippy` for linting recommendations
- Write self-documenting code with clear variable/function names
- Add doc comments (`///`) for public APIs

## Pull Request Process

1. Fork the repository
2. Create your feature branch
3. Make your changes with adequate tests
4. Ensure all tests pass and clippy is clean
5. Submit a pull request with a clear description

## Reporting Issues

- Use GitHub Issues for bug reports and feature requests
- Include Rust version (`rustc --version`) and OS information
- For bugs, provide minimal reproduction steps

## License

By contributing, you agree that your contributions will be licensed under the project's [GNU Affero General Public License v3.0](LICENSE), using the SPDX identifier `AGPL-3.0-only`.
