# Makefile for KeepActive

# Compiler and flags
CC = gcc
CFLAGS = -Wall -Wextra -std=c99
LIBS = -lpthread -lws2_32
TARGET = KeepActive.exe
SOURCE = KeepActive.c

# Default target
all: $(TARGET)

# Build the executable
$(TARGET): $(SOURCE)
	$(CC) $(CFLAGS) -o $(TARGET) $(SOURCE) $(LIBS)

# Clean build artifacts
clean:
	@if [ -f $(TARGET) ]; then rm -f $(TARGET) && echo "Cleaned build artifacts"; else echo "Nothing to clean"; fi

# Install dependencies (placeholder for future use)
install:
	@echo No dependencies to install

# Run the program with default settings
run: $(TARGET)
	./$(TARGET)

# Display help
help:
	@echo "Available targets:"
	@echo "  all      - Build the executable (default)"
	@echo "  clean    - Remove build artifacts"
	@echo "  install  - Install dependencies (none currently)"
	@echo "  run      - Build and run the program"
	@echo "  help     - Show this help message"

.PHONY: all clean install run help