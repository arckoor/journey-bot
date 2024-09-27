/*
  Warnings:

  - You are about to drop the column `mute_role` on the `AntiSpamConfig` table. All the data in the column will be lost.

*/
-- AlterTable
ALTER TABLE "AntiSpamConfig" DROP COLUMN "mute_role",
ADD COLUMN     "clean_user" BOOLEAN NOT NULL DEFAULT false,
ADD COLUMN     "timeout_duration" INTEGER;
