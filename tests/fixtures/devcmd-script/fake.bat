@echo off
rem Fake VsDevCmd.bat for prepare-devenv test_hooks integration tests.
rem Sets known env vars then optionally exits with --exit-code N.

if "%~1"=="--exit-code" (
    exit /b %~2
)

set FAKE_INCLUDE=C:\fake\include
set FAKE_LIB=C:\fake\lib
set FAKE_VCINSTALLDIR=C:\fake\vc
exit /b 0