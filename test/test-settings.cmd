@echo off
REM ============================================================
REM  test-settings.cmd
REM
REM  Test verschillende tci-streamer client-instellingen na elkaar.
REM  Per test draait de client een vaste tijd; jij luistert en
REM  beoordeelt. Druk een toets om naar de volgende test te gaan.
REM
REM  Pas SERVER en LISTEN hieronder aan voor je eigen setup.
REM  De server moet al draaien voordat je dit script start.
REM ============================================================

setlocal enabledelayedexpansion
cd /d "%~dp0"

REM ---- Pas dit aan ----
set SERVER=ws://192.168.2.117:9001
set LISTEN=127.0.0.1:40002
set EXE=tci-streamer-client.exe
REM ---------------------

if not exist "%EXE%" (
    echo FOUT: %EXE% niet gevonden in deze map.
    echo Zet dit script in dezelfde map als de client, of pas EXE aan.
    pause
    exit /b 1
)

echo ==========================================================
echo   tci-streamer instellingen-test
echo ==========================================================
echo   Server : %SERVER%
echo   Listen : %LISTEN%
echo.
echo   Per test start de client. Verbind THRA met %LISTEN%,
echo   luister, en sluit dan het client-venster (of Ctrl+C)
echo   om naar de volgende test te gaan.
echo ==========================================================
echo.
pause

REM Elke test: label + argumenten. We starten de client in een
REM nieuw venster en wachten tot jij het sluit.

call :runtest "1. Lossless (referentie, geen compressie)" ^
    "--codec lossless"

call :runtest "2. FLAC mono (lossless ~700kbps)" ^
    "--codec flac --channels mono"

call :runtest "3. Opus mono 24k (SSB standaard)" ^
    "--codec opus --sample-rate 48000 --frame-ms 20 --bitrate 24000 --channels mono"

call :runtest "4. Opus mono 32k (iets hogere kwaliteit)" ^
    "--codec opus --sample-rate 48000 --frame-ms 20 --bitrate 32000 --channels mono"

call :runtest "5. Opus mono 16k smalband (zuinig)" ^
    "--codec opus --sample-rate 16000 --frame-ms 40 --bitrate 16000 --channels mono"

call :runtest "6. Opus mono 24k + SSB filter" ^
    "--codec opus --bitrate 24000 --channels mono --rx-filter ssb"

call :runtest "7. Opus stereo 96k (hi-fi)" ^
    "--codec opus --sample-rate 48000 --frame-ms 20 --bitrate 96000 --channels stereo"

echo.
echo ==========================================================
echo   Alle tests klaar.
echo ==========================================================
pause
exit /b 0

REM ---- subroutine: runtest "naam" "args" ----
:runtest
echo.
echo ----------------------------------------------------------
echo   TEST: %~1
echo   args: %~2
echo ----------------------------------------------------------
echo   Start de client. Sluit het venster om door te gaan.
echo.
REM /wait zorgt dat we wachten tot het client-venster sluit
start "TCI test: %~1" /wait %EXE% --server %SERVER% --listen %LISTEN% %~2
echo   Test afgerond: %~1
echo.
echo   Druk een toets voor de volgende test (of sluit dit venster om te stoppen)...
pause >nul
goto :eof
