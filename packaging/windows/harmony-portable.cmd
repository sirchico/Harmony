@echo off
rem Runs Harmony with its data beside this file. Security software can deny an unsigned build any
rem write inside the user profile (ERROR_ACCESS_DENIED), and then the cache, saved logins and the
rem diagnostic log all stay read-only. Put this file next to harmony.exe and start Harmony with it.
setlocal
set "ROOT=%~dp0"
if not exist "%ROOT%harmony.exe" (
	echo Put this file next to harmony.exe.
	exit /b 1
)
if not exist "%ROOT%data" mkdir "%ROOT%data"
set "HARMONY_DATA_DIR=%ROOT%data"
start "" "%ROOT%harmony.exe" %*
endlocal
