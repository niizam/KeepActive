# CI/CD Documentation

## GitHub Actions Workflow

This repository includes a GitHub Actions workflow for building and releasing the KeepActive binary.

### Triggering a Release

1. Go to the GitHub repository's **Actions** tab
2. Select the "Build and Release" workflow
3. Click "Run workflow"
4. Fill in the required inputs:
   - **Release tag**: Version tag (e.g., `v1.0.0`, `v1.1.0`)
   - **Release name**: Human-readable release name (optional, defaults to "KeepActive Release")
5. Click "Run workflow" to start the build

### What the Workflow Does

The workflow performs the following steps:

1. **Checkout Code**: Downloads the repository source code
2. **Setup MinGW**: Installs GCC compiler with Windows libraries support
3. **Build KeepActive**: Compiles the C source code into `KeepActive.exe`
4. **Verify Build**: Checks that the binary was created successfully
5. **Create Release**: Creates a GitHub release with the specified tag and uploads the binary

### Build Requirements

- **Runner**: Windows Latest (windows-latest)
- **Compiler**: GCC with MinGW
- **Libraries**: pthread, ws2_32
- **Output**: `KeepActive.exe` (Windows executable)

### Release Assets

Each release will include:
- `KeepActive.exe` - The compiled binary ready to run on Windows
- Release notes with usage instructions and requirements

### Local Development

For local development, you can use the included Makefile:

```bash
# Build the executable
make

# Clean build artifacts
make clean

# Build and run
make run

# Show help
make help
```

### Manual Build

If you prefer to build manually without the Makefile:

```bash
gcc -o KeepActive.exe KeepActive.c -lpthread -lws2_32
```