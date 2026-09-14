PROFILE?=debug
RUST_LIB=target/$(PROFILE)/libio.a
RUST_SOURCES=$(wildcard src/*.rs crates/*/src/*.rs crates/*/src/physics/*.rs)
RUST_MANIFESTS=Cargo.toml $(wildcard crates/*/Cargo.toml)
BUILD_DIR=build$(if $(filter release,$(PROFILE)),/release)
APP=$(BUILD_DIR)/io
CC=cc
CFLAGS=-std=c11 -Wall -Wextra -Werror $(if $(filter release,$(PROFILE)),-O2,-g) -Iinclude -I/opt/homebrew/include -DGL_SILENCE_DEPRECATION
LDFLAGS=-L/opt/homebrew/lib -Wl,-rpath,/opt/homebrew/lib -lSDL2 -framework OpenGL -Wl,-sectcreate,__TEXT,__info_plist,csrc/Info.plist
RENDER_SOURCES=csrc/renderer.c csrc/dynamic_buffer.c csrc/hud.c
RENDER_HEADERS=csrc/renderer.h csrc/dynamic_buffer.h csrc/hud.h

.PHONY: all run test gpu-test release smoke clean

all: $(APP)

$(RUST_LIB): $(RUST_MANIFESTS) $(RUST_SOURCES)
	cargo build -p io --lib $(if $(filter release,$(PROFILE)),--release)

$(APP): $(RUST_LIB) csrc/main.c csrc/benchmark.c csrc/benchmark.h csrc/input.c csrc/input.h $(RENDER_SOURCES) $(RENDER_HEADERS) csrc/Info.plist include/io.h Makefile
	mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) csrc/main.c csrc/benchmark.c csrc/input.c $(RENDER_SOURCES) $(RUST_LIB) -o $(APP) $(LDFLAGS)

run: $(APP)
	./$(APP)

$(BUILD_DIR)/input-test: $(RUST_LIB) tests/input_test.c csrc/input.c csrc/input.h include/io.h csrc/Info.plist Makefile
	mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) -Werror tests/input_test.c csrc/input.c $(RUST_LIB) -o $@ $(LDFLAGS)

$(BUILD_DIR)/hud-test: tests/hud_test.c csrc/hud.c csrc/hud.h include/io.h Makefile
	mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) tests/hud_test.c csrc/hud.c -o $@ -framework OpenGL

$(BUILD_DIR)/game-test: $(RUST_LIB) tests/game_test.c csrc/input.c csrc/input.h include/io.h Makefile
	mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) tests/game_test.c csrc/input.c $(RUST_LIB) -o $@ $(LDFLAGS)

$(BUILD_DIR)/village-test: $(RUST_LIB) tests/village_test.c csrc/input.c csrc/input.h include/io.h Makefile
	mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) tests/village_test.c csrc/input.c $(RUST_LIB) -o $@ $(LDFLAGS)

test: $(BUILD_DIR)/input-test $(BUILD_DIR)/hud-test $(BUILD_DIR)/game-test $(BUILD_DIR)/village-test
	cargo test --workspace
	./$(BUILD_DIR)/input-test
	./$(BUILD_DIR)/hud-test
	./$(BUILD_DIR)/game-test
	./$(BUILD_DIR)/village-test
	python3 -m unittest discover -s tests -p 'test_*.py'

release:
	$(MAKE) PROFILE=release all

$(BUILD_DIR)/renderer-buffer-test: $(RUST_LIB) tests/renderer_buffer_test.c $(RENDER_SOURCES) $(RENDER_HEADERS) include/io.h Makefile
	mkdir -p $(BUILD_DIR)
	$(CC) $(CFLAGS) tests/renderer_buffer_test.c $(RENDER_SOURCES) $(RUST_LIB) -o $@ $(LDFLAGS)

gpu-test: $(BUILD_DIR)/renderer-buffer-test
	./$(BUILD_DIR)/renderer-buffer-test
	./$(BUILD_DIR)/renderer-buffer-test --variants
	./$(BUILD_DIR)/renderer-buffer-test --game

smoke: $(APP)
	./$(APP) --smoke-test

clean:
	cargo clean
	rm -rf build
