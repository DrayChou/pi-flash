@echo off
setlocal
cd /d "%~dp0"

rem ============================================================
rem  clean_cache.bat -- repo-root double-click entry: SAFE tier.
rem
rem  Equivalent to:  python scripts\clean-cache.py --apply
rem
rem  Safe tier only deletes STALE hashes of final artifacts
rem  (target\<profile>\deps\*.exe / *.pdb) plus tmp drafts older
rem  than 3 days and stray cargo target dirs. cargo rebuilds the
rem  deleted exe/pdb on the next build, so the cost is zero.
rem  It NEVER touches rlib/rmeta/lib/dll (shared by several bins);
rem  run scripts\clean-cache.py --apply --aggressive by hand for that.
rem
rem  This file is ASCII-only ON PURPOSE: a batch file holding
rem  non-ASCII text breaks cmd's line parser once the console code
rem  page changes inside the script (mojibake + bogus commands).
rem  Chinese output comes from the Python side, with the console
rem  switched to UTF-8 below (done in a child cmd, so THIS file is
rem  never parsed under two different code pages).
rem ============================================================

set "PY="
where python >nul 2>&1 && set "PY=python"
if not defined PY where py >nul 2>&1 && set "PY=py"
if not defined PY (
    echo [x] python not found in PATH. Install Python 3 first.
    goto :end
)

cmd /c chcp 65001 >nul 2>&1

echo.
echo ==== STEP 1/2  dry-run preview (nothing is deleted) ====
%PY% scripts\clean-cache.py --dry-run
if errorlevel 1 goto :end

echo.
set "ANS="
set /p "ANS=Delete the items listed above? [y/N] "
if /i not "%ANS%"=="y" (
    echo Cancelled. Nothing was deleted.
    goto :end
)

echo.
echo ==== STEP 2/2  cleaning (safe tier) ====
%PY% scripts\clean-cache.py --apply

:end
echo.
echo ------------------------------------------------------------
echo Log: target\clean-cache.log   (dry-run: clean-cache.dry.log)
echo ------------------------------------------------------------
pause
endlocal
