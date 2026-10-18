# Reproducible release build of the Tanqi desktop binary (Dioxus + WebKitGTK).
# Nothing is run in the container; the binary is exported with:
#   docker build --target artifact --output type=local,dest=dist .
# The result (dist/tanqi-music) is a dynamically linked Linux binary: the machine
# running it needs the WebKitGTK 4.1 / GTK3 runtime libs (libwebkit2gtk-4.1-0,
# libgtk-3-0, libxdo3).
# Build on the oldest distro you want to run on - Ubuntu 24.04 here.
FROM ubuntu:24.04 AS build
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential pkg-config curl ca-certificates git \
        libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev libssl-dev libdbus-1-dev \
    && rm -rf /var/lib/apt/lists/*
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH=/root/.cargo/bin:$PATH
WORKDIR /app

# Dependency layer: build with empty sources first so editing src/ doesn't
# recompile every crate. build.rs needs the CSS/fonts, so they come along.
COPY Cargo.toml Cargo.lock build.rs ./
COPY assets/css assets/css
COPY assets/fonts assets/fonts
RUN mkdir src && touch src/lib.rs && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked; \
    rm -rf src

COPY . .
# Make sure cargo sees the real sources as newer than the dummy ones.
RUN touch src/lib.rs src/main.rs && cargo build --release --locked

FROM scratch AS artifact
COPY --from=build /app/target/release/tanqi-music /tanqi-music

# Runnable image (last stage, so a plain `docker build -t tanqi-music .` gives this).
# It is a GUI app: it needs your display, audio and session bus passed in -
# see docker-run.sh.
FROM ubuntu:24.04 AS runtime
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        libwebkit2gtk-4.1-0 libgtk-3-0 libxdo3 libdbus-1-3 ca-certificates \
        gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav gstreamer1.0-pulseaudio \
        libgl1 libgl1-mesa-dri fonts-noto-core fonts-noto-color-emoji adwaita-icon-theme \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/tanqi-music /usr/local/bin/tanqi-music
# The image's `ubuntu` user is uid 1000, matching a typical host user so the
# X11/Pulse/D-Bus sockets are accessible. Data lives in a volume at /data.
RUN mkdir -p /data && chown 1000:1000 /data
USER 1000
ENV XDG_DATA_HOME=/data HOME=/home/ubuntu
VOLUME /data
CMD ["tanqi-music"]
