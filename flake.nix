{
  description = "flare devshell";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    systems.url = "github:nix-systems/default";
    flake-utils = {
      url = "github:numtide/flake-utils";
      inputs.systems.follows = "systems";
    };
  };

  outputs = {
    nixpkgs,
    flake-utils,
    ...
  } @ inputs:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {
          inherit system;
        };
      in
        with pkgs; let
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

            (mkScript "db-stop" ''
              pg_ctl -D $PGDATA stop 2> /dev/null
            '')

            (mkScript "format" "black -l 120 .")
          ];

          aiosqlite = python313Packages.buildPythonPackage rec {
            pname = "aiosqlite";
            version = "0.17.0";
            format = "pyproject";
            src = fetchPypi {
              inherit pname version;
              sha256 = "sha256-8OaswkvEhkFJJnrIL7Rt+zvkRV+Z/iHfgmCcxua67lE=";
            };
            build-system = with python313Packages; [
              flit-core
            ];
            dependencies = with python313Packages; [
              typing-extensions
            ];
          };
          asyncprawcore = python313Packages.buildPythonPackage rec {
            pname = "asyncprawcore";
            version = "2.4.0";
            format = "pyproject";
            src = fetchPypi {
              inherit pname version;
              sha256 = "sha256-OjNZ5c0evmHVRKCdS17Kexbt/RfeBwgahBX8eU97ti4=";
            };
            build-system = with python313Packages; [
              flit-core
            ];
            dependencies = with python313Packages; [
              yarl
              aiohttp
            ];
          };
          asyncpraw = python313Packages.buildPythonPackage rec {
            pname = "asyncpraw";
            version = "7.8.1";
            format = "pyproject";
            src = fetchPypi {
              inherit pname version;
              sha256 = "sha256-b8UOOXauEG72GQ28yjsbQFDefaVkStxQhRrmSHZ5IG8=";
            };
            build-system = with python313Packages; [
              flit-core
            ];
            dependencies = with python313Packages; [
              aiofiles
              aiohttp
              asyncprawcore
              update-checker
              aiosqlite
            ];
          };

          pypika-tortoise = python313Packages.buildPythonPackage rec {
            pname = "pypika-tortoise";
            version = "0.5.0";
            format = "pyproject";
            src = fetchFromGitHub {
              owner = "tortoise";
              repo = "pypika-tortoise";
              rev = "1ab383c9def600a271d2d4d17ea04d0e491cb672";
              sha256 = "sha256-QkNhxmXupRiZ41hZw9eZ1dJT82W+fXJ5uKqjkOOev0E=";
            };

            build-system = with python313Packages; [
              poetry-core
            ];
          };

          tortoise-orm = python313Packages.buildPythonPackage rec {
            pname = "tortoise_orm";
            version = "0.24.2";
            format = "pyproject";
            src = fetchPypi {
              inherit pname version;
              sha256 = "sha256-g9I3Pvr8HF1mjDxb1OHn9KIZUm6FCQn/6I8YTaOAj9s=";
            };
            build-system = with python313Packages; [
              poetry-core
            ];
            dependencies = with python313Packages; [
              iso8601
              aiosqlite
              pytz
              pypika-tortoise
              asyncpg
            ];
          };

          aerich = python313Packages.buildPythonPackage rec {
            pname = "aerich";
            version = "0.8.2";
            format = "pyproject";
            src = fetchPypi {
              inherit pname version;
              sha256 = "sha256-DtKxW7AXeF0XMjMp5USLSxL8/sCKIJMmJsEqpZqDeNY=";
            };
            build-system = with python313Packages; [
              poetry-core
            ];
            dependencies = with python313Packages; [
              pydantic
              tortoise-orm
              asyncclick
              asyncpg
              dictdiffer
              tomlkit
            ];
          };
        in {
          devShells.default = pkgs.mkShell {
            packages = with pkgs;
              [
                (python313.withPackages (
                  ps:
                    with ps; [
                      aerich
                      asyncpraw
                      colorama
                      disnake
                      flake8
                      levenshtein
                      tortoise-orm
                      twitchapi
                      black
                    ]
                ))

                openssl
                postgresql
              ]
              ++ shellScripts;

            shellHook = ''
              export NIX_SHELL_DIR="$PWD/.nix-shell"
              export PGDATA="$NIX_SHELL_DIR/postgres"
            '';
          };
        }
    );
}
