#!/usr/bin/bash

set -e

if [ "$ENV" = "dev" ]; then
	prisma db push
else
	prisma migrate deploy
fi

exec "$@"
