{pkgs ? import <nixpkgs> {}}:
with pkgs; let
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
in
  pkgs.mkShell {
    buildInputs = with pkgs; [
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
          ]
      ))

      black
      openssl
      postgresql
    ];

    shellHook = ''
      alias black="black .";

      export PYTHONPATH=$(python -c "import site; print(site.getsitepackages()[0])")
      export NIX_SHELL_DIR="$PWD/.nix-shell"
      export PGDATA="$NIX_SHELL_DIR/postgres"

      export DATABASE_BASE=postgres://journey-bot:12345@localhost:5432


      if ! test -d $PGDATA; then
        pg_ctl initdb -D  $PGDATA
      fi

      HOST_COMMON="host\s\+all\s\+all"
      sed -i "s|^$HOST_COMMON.*127.*$|host all all 127.0.0.1/32 trust|" $PGDATA/pg_hba.conf
      sed -i "s|^$HOST_COMMON.*::1.*$|host all all ::1/128 trust|"      $PGDATA/pg_hba.conf

      pg_ctl                                                  \
      -D $PGDATA                                              \
      -l $PGDATA/postgres.log                                 \
      -o "-c unix_socket_directories='$PGDATA'"               \
      -o "-c listen_addresses='localhost'"                    \
      -o "-c log_destination='stderr'"                        \
      -o "-c logging_collector=on"                            \
      -o "-c log_directory='log'"                             \
      -o "-c log_filename='postgresql-%Y-%m-%d_%H-%M-%S.log'" \
      -o "-c log_min_messages=info"                           \
      -o "-c log_min_error_statement=info"                    \
      -o "-c log_connections=on"                              \
      start

      psql -h $PGDATA -d postgres -c "CREATE USER \"journey-bot\" WITH PASSWORD '12345' CREATEDB;"
      psql -h $PGDATA -d postgres -c "CREATE DATABASE \"journey-bot-db\" OWNER \"journey-bot\";"

      trap 'pg_ctl -D $PGDATA stop 2> /dev/null' EXIT
    '';
  }
