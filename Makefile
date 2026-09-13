PROFILE?=debug
RUST_LIB=target/$(PROFILE)/libio.a
RUST_SOURCES=$(wildcard src/*.rs crates/*/src/*.rs)
RUST_MANIFESTS=Cargo.toml $(wildcard crates/*/Cargo.toml)
BUILD_DIR=build$(if $(filter release,$(PROFILE)),/release)
APP=$(BUILD_DIR)/io
CC=cc
CFLAGS=-std=c11 -Wall -Wextra -Werror $(if $(filter release,$(PROFILE)),-O2,-g) -Iinclude -I/opt/homebrew/include -DGL_SILENCE_DEPRECATION
LDFLAGS=-L/opt/homebrew/lib -Wl,-rpath,/opt/homebrew/lib -lSDL2 -framework OpenGL -Wl,-sectcreate,__TEXT,__info_plist,csrc/Info.plist

.PHONY: all run test gpu-test release smoke clean

all: $(APP)

$(RUST_LIB): $(RUST_MANIFESTS) $(RUST_SOURCES)
	cargo build -p io --lib $(if $(filter release,$(PROFILE)),--release)

$(APP): $(RUST_LIB) csrc/main.c csrc/benchmark.c csrc/benchmark.h csrc/input.c csrc/input.h csrc/renderer.c csrc/renderer.h csrc/dynamic_buffer.c csrc/dynamic_buffer.h csrc/Info.plist include/io.h Makefile
	mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) csrc/main.c csrc/benchmark.c csrc/input.c csrc/renderer.c csrc/dynamic_buffer.c $(RUST_LIB) -o $(APP) $(LDFLAGS)

run: $(APP)
	./$(APP)

$(BUILD_DIR)/input-test: $(RUST_LIB) tests/input_test.c csrc/input.c csrc/input.h include/io.h csrc/Info.plist Makefile
	mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) -Werror tests/input_test.c csrc/input.c $(RUST_LIB) -o $@ $(LDFLAGS)

test: $(BUILD_DIR)/input-test
	cargo test --workspace
	./$(BUILD_DIR)/input-test
	python3 -m unittest discover -s tests -p test_benchmark.py

release:
	$(MAKE) PROFILE=release all

$(BUILD_DIR)/renderer-buffer-test: $(RUST_LIB) tests/renderer_buffer_test.c csrc/renderer.c csrc/renderer.h csrc/dynamic_buffer.c csrc/dynamic_buffer.h include/io.h Makefile
	mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) tests/renderer_buffer_test.c csrc/renderer.c csrc/dynamic_buffer.c $(RUST_LIB) -o $@ $(LDFLAGS)

gpu-test: $(BUILD_DIR)/renderer-buffer-test
	./$(BUILD_DIR)/renderer-buffer-test
	./$(BUILD_DIR)/renderer-buffer-test --variants

smoke: $(APP)
	./$(APP) --smoke-test

clean:
	cargo clean
	rm -rf build
