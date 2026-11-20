@echo off
rem Toolchain Universal Mobile Runtime: finds tumr.exe without PATH and runs it with the given arguments.
rem Order: TUMR_EXE, the default install folder, then tumr.exe on PATH.
setlocal
if defined TUMR_EXE if exist "%TUMR_EXE%" (set "TUMR=%TUMR_EXE%" & goto run)
set "TUMR=%LOCALAPPDATA%\Toolchain Universal Mobile Runtime\tumr.exe"
if exist "%TUMR%" goto run
for /f "delims=" %%i in ('where tumr.exe 2^>nul') do (set "TUMR=%%i" & goto run)
echo Toolchain Universal Mobile Runtime is not installed on this PC. Install it from https://github.com/Jishnu2608/TUMR/releases, open it once, then restart this agent. 1>&2
exit /b 9009
:run
"%TUMR%" %*
exit /b %ERRORLEVEL%
