#!/bin/bash

# Simple validation script for the CI/CD setup

echo "=== KeepActive CI/CD Validation ==="
echo

# Check if required files exist
echo "Checking required files..."
files=(".github/workflows/build-and-release.yml" "KeepActive.c" "Makefile")
all_files_exist=true

for file in "${files[@]}"; do
    if [ -f "$file" ]; then
        echo "✓ $file exists"
    else
        echo "✗ $file missing"
        all_files_exist=false
    fi
done

echo

# Validate YAML syntax
echo "Validating workflow YAML syntax..."
if python3 -c "import yaml; yaml.safe_load(open('.github/workflows/build-and-release.yml'))" 2>/dev/null; then
    echo "✓ Workflow YAML syntax is valid"
else
    echo "✗ Workflow YAML syntax error or PyYAML not available"
fi

echo

# Test Makefile
echo "Testing Makefile..."
if make help >/dev/null 2>&1; then
    echo "✓ Makefile syntax is valid"
else
    echo "✗ Makefile has syntax errors"
fi

echo

# Check workflow trigger configuration
echo "Checking workflow configuration..."
if grep -q "workflow_dispatch:" .github/workflows/build-and-release.yml; then
    echo "✓ Workflow has manual trigger (workflow_dispatch)"
else
    echo "✗ Workflow missing manual trigger"
fi

if grep -q "windows-latest" .github/workflows/build-and-release.yml; then
    echo "✓ Workflow uses Windows runner"
else
    echo "✗ Workflow not configured for Windows"
fi

if grep -q "gcc.*KeepActive.exe.*KeepActive.c.*lpthread.*lws2_32" .github/workflows/build-and-release.yml; then
    echo "✓ Workflow has correct build command"
else
    echo "✗ Workflow missing correct build command"
fi

if grep -q "gh release create" .github/workflows/build-and-release.yml; then
    echo "✓ Workflow creates GitHub release"
else
    echo "✗ Workflow missing release creation"
fi

echo

if $all_files_exist; then
    echo "🎉 CI/CD setup validation completed successfully!"
    echo
    echo "To trigger a release:"
    echo "1. Go to GitHub Actions tab"
    echo "2. Select 'Build and Release' workflow" 
    echo "3. Click 'Run workflow'"
    echo "4. Enter release tag (e.g., v1.0.0) and run"
else
    echo "❌ Validation failed - some required files are missing"
    exit 1
fi