{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    systems.url = "github:nix-systems/default";
    rust-overlay.url = "github:oxalica/rust-overlay";
    naersk.url = "github:nix-community/naersk";
    flake-utils = {
      url = "github:numtide/flake-utils";
      inputs.systems.follows = "systems";
    };
  };

  outputs = {
    nixpkgs,
    rust-overlay,
    naersk,
    flake-utils,
    ...
  } @ inputs:
    flake-utils.lib.eachSystem ["x86_64-linux"] (
      system: let
        overlays = [(import rust-overlay)];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        mkScript = name: text: (pkgs.writeShellScriptBin name text);

        shellScripts = [
          (mkScript "db-setup" ''
            if ! test -d $PGDATA; then
              pg_ctl initdb -D $PGDATA
            fi

            HOST_COMMON="host\s\+all\s\+all"
            sed -i "s|^$HOST_COMMON.*127.*$|host all all 127.0.0.1/32 trust|" $PGDATA/pg_hba.conf
            sed -i "s|^$HOST_COMMON.*::1.*$|host all all ::1/128 trust|"      $PGDATA/pg_hba.conf
          '')

          (mkScript "db-reset" ''
            db-stop
            rm -rf $PGDATA
            db-start
          '')

          (mkScript "db-start" ''
            db-setup

            pg_ctl                                                  \
            -D $PGDATA                                              \
            -l $PGDATA/postgres.log                                 \
            -o "-c unix_socket_directories='$PGDATA'"               \
            -o "-c listen_addresses='localhost'"                    \
            start

            psql -h $PGDATA -d postgres -c "CREATE USER \"journey-bot\" WITH PASSWORD '12345' CREATEDB;"
            psql -h $PGDATA -d postgres -c "CREATE DATABASE \"journey-bot-db\" OWNER \"journey-bot\";"
          '')

          (mkScript "gen" ''
            sea-orm-cli generate entity \
              -u postgres://journey-bot:12345@localhost:5432/journey-bot-db \
              -o sea-entity/src \
              --lib \
              --with-prelude none \
              --with-copy-enums
          '')
          (mkScript "db-stop" ''
            pg_ctl -D $PGDATA stop 2> /dev/null
          '')

          (mkScript "mig" "sea-orm-cli migrate -d sea-migration generate")
        ];

        naersk' = pkgs.callPackage naersk {};

        journey-bot = naersk'.buildPackage {
          src = ./.;
        };
      in {
        devShells.default = pkgs.mkShell {
          packages = with pkgs;
            [
              rust-bin.stable.latest.default
              cargo-edit
              sea-orm-cli

              postgresql
            ]
            ++ shellScripts;

          RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";

          shellHook = ''
            export NIX_SHELL_DIR="$PWD/.nix-shell"
            export PGDATA="$NIX_SHELL_DIR/postgres"
          '';
        };

        packages.default = pkgs.dockerTools.buildImage {
          name = "journey-bot";
          tag = "latest";
          created = "now";
          copyToRoot = pkgs.buildEnv {
            name = "image-root";
            paths = [
              journey-bot
            ];
            pathsToLink = ["/bin" "/journey-bot"];
          };

          config = {
            WorkingDir = "/journey-bot";
            Cmd = ["journey-bot"];
          };
        };
      }
    );
}
