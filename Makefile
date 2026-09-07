RUST_LIB=target/debug/libio.a
APP=build/io
CC=cc
CFLAGS=-std=c11 -Wall -Wextra -Iinclude -I/opt/homebrew/include -DGL_SILENCE_DEPRECATION
LDFLAGS=-L/opt/homebrew/lib -Wl,-rpath,/opt/homebrew/lib -lSDL2 -framework OpenGL -Wl,-sectcreate,__TEXT,__info_plist,csrc/Info.plist

.PHONY: all run clean

all: $(APP)

$(RUST_LIB): Cargo.toml src/lib.rs src/scene.rs src/space.rs src/camera.rs
	cargo build --lib

$(APP): $(RUST_LIB) csrc/main.c csrc/renderer.c csrc/renderer.h csrc/Info.plist include/io.h Makefile
	mkdir -p build
	$(CC) $(CFLAGS) csrc/main.c csrc/renderer.c $(RUST_LIB) -o $(APP) $(LDFLAGS)

run: $(APP)
	./$(APP)

clean:
	cargo clean
	rm -rf build
