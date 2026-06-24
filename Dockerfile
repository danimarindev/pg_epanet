# Stage 1: build the extension against PostgreSQL 18
FROM rust:1-bookworm AS builder

RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    cmake \
    libclang-dev \
    pkg-config \
    && install -d /usr/share/postgresql-common/pgdg \
    && curl -o /usr/share/postgresql-common/pgdg/apt.postgresql.org.asc --fail \
        https://www.postgresql.org/media/keys/ACCC4CF8.asc \
    && printf 'Types: deb\nURIs: https://apt.postgresql.org/pub/repos/apt\nSuites: bookworm-pgdg\nComponents: main\nSigned-By: /usr/share/postgresql-common/pgdg/apt.postgresql.org.asc\n' \
        > /etc/apt/sources.list.d/pgdg.sources \
    && apt-get update \
    && apt-get install -y postgresql-18 postgresql-server-dev-18 \
    && rm -rf /var/lib/apt/lists/*

RUN cargo install cargo-pgrx --version '=0.19.1'

ENV PGRX_HOME=/root/.pgrx
RUN cargo pgrx init --pg18 /usr/lib/postgresql/18/bin/pg_config

WORKDIR /build
COPY . .

RUN cargo pgrx install --release --features pg18 --no-default-features \
    --pg-config /usr/lib/postgresql/18/bin/pg_config

# Stage 2: runtime — postgres:18-trixie has arm64; postgis/postgis:18-3.6 is amd64-only
FROM postgres:18-trixie

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        postgresql-18-postgis-3 \
        postgresql-18-postgis-3-scripts \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/share/postgresql/18/extension/pg_epanet* \
    /usr/share/postgresql/18/extension/
COPY --from=builder /usr/lib/postgresql/18/lib/pg_epanet.so \
    /usr/lib/postgresql/18/lib/

COPY docker/initdb.d/01-pg-epanet.sh /docker-entrypoint-initdb.d/01-pg-epanet.sh
RUN chmod +x /docker-entrypoint-initdb.d/01-pg-epanet.sh
