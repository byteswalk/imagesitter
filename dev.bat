@echo off
title ImageSitter Dev
cd /d %~dp0
call pnpm tauri dev
echo.
echo Dev process has exited.
pause
