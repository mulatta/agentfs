# Makefile for AgentFS FSKit build
#
# This coordinates building the Rust FFI library and Swift FSKit extension.

.PHONY: all clean build-ffi build-extension install-extension help

# Detect platform
UNAME := $(shell uname)
ARCH := $(shell uname -m)

# Paths
FFI_DIR := .
EXTENSION_DIR := ../fskit-extension
FFI_HEADER := $(FFI_DIR)/agentfs_ffi.h
FFI_LIB := $(FFI_DIR)/target/release/libagentfs_ffi.a

# Default target
all: build-ffi build-extension

# Build the Rust FFI library
build-ffi:
	@echo "Building Rust FFI library..."
	cargo build --release
	@echo "Copying header to Swift extension..."
	cp $(FFI_HEADER) $(EXTENSION_DIR)/Sources/AgentFSExtension/

# Build the Swift FSKit extension (macOS only)
build-extension: build-ffi
ifeq ($(UNAME),Darwin)
	@echo "Building Swift FSKit extension..."
	cd $(EXTENSION_DIR) && swift build -c release \
		-Xswiftc -I$(shell pwd) \
		-Xlinker -L$(shell pwd)/target/release \
		-Xlinker -lagentfs_ffi
else
	@echo "Skipping FSKit extension build (not on macOS)"
endif

# Clean build artifacts
clean:
	cargo clean
ifeq ($(UNAME),Darwin)
	cd $(EXTENSION_DIR) && swift package clean || true
endif

# Install the extension (requires code signing)
install-extension: build-extension
ifeq ($(UNAME),Darwin)
	@echo ""
	@echo "=============================================="
	@echo "FSKit Extension Built Successfully!"
	@echo "=============================================="
	@echo ""
	@echo "To install the extension:"
	@echo "1. The extension needs to be code-signed and notarized"
	@echo "2. Package it in an app bundle for distribution"
	@echo "3. Users enable it via:"
	@echo "   System Settings > General > Login Items & Extensions"
	@echo "   > File System Extensions"
	@echo ""
	@echo "For development, you can run directly from Xcode with"
	@echo "proper signing configuration."
	@echo ""
else
	@echo "FSKit extensions only work on macOS"
endif

# Help
help:
	@echo "AgentFS FSKit Build System"
	@echo ""
	@echo "Targets:"
	@echo "  all              - Build FFI library and Swift extension"
	@echo "  build-ffi        - Build Rust FFI library only"
	@echo "  build-extension  - Build Swift FSKit extension"
	@echo "  clean            - Clean all build artifacts"
	@echo "  install-extension - Show installation instructions"
	@echo "  help             - Show this help"
	@echo ""
	@echo "Requirements:"
	@echo "  - Rust toolchain (cargo)"
	@echo "  - Swift 6.0+ (for FSKit extension)"
	@echo "  - macOS 15+ SDK (for FSKit framework)"
