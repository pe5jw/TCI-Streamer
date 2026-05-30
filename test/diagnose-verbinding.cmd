@echo off
REM ============================================================
REM  diagnose-verbinding.cmd
REM
REM  Diagnosticeert het "os error 10060" verbindingsprobleem
REM  tussen tci-streamer client en server.
REM
REM  Draai dit op de CLIENT-PC. Pas SERVER_IP en PORT aan.
REM ============================================================

setlocal
REM ---- Pas dit aan ----
set SERVER_IP=192.168.2.117
set PORT=9001
REM ---------------------

echo ==========================================================
echo   tci-streamer verbindings-diagnose
echo   Doel: %SERVER_IP%:%PORT%
echo ==========================================================
echo.

echo [1] Ping naar de server...
ping -n 3 %SERVER_IP%
if errorlevel 1 (
    echo.
    echo   ^>^> PING MISLUKT. De server-PC is niet bereikbaar.
    echo      Mogelijke oorzaken:
    echo        - Verkeerd IP-adres ^(check met 'ipconfig' op de server-PC^)
    echo        - Server-PC staat uit of op een ander netwerk
    echo        - Beide PC's op verschillend subnet ^(192.168.2.x vs 192.168.8.x?^)
    echo.
    goto :end
)
echo   ^>^> Ping OK: de server-PC is bereikbaar.
echo.

echo [2] Test TCP-poort %PORT%...
powershell -NoProfile -Command "$r = Test-NetConnection -ComputerName '%SERVER_IP%' -Port %PORT% -WarningAction SilentlyContinue; if ($r.TcpTestSucceeded) { Write-Host '   >> POORT OPEN: verbinding met %PORT% lukt.' -ForegroundColor Green } else { Write-Host '   >> POORT DICHT/GEBLOKKEERD op %PORT%.' -ForegroundColor Red }"
echo.

echo ==========================================================
echo   Interpretatie
echo ==========================================================
echo   - Ping OK + poort open   : netwerk is prima, probleem ligt elders.
echo                              ^(draait de server echt? juiste --listen?^)
echo   - Ping OK + poort dicht   : FIREWALL op de server-PC blokkeert %PORT%.
echo                              Draai op de SERVER-PC als administrator:
echo.
echo        netsh advfirewall firewall add rule name="tci-streamer" ^
echo            dir=in action=allow protocol=TCP localport=%PORT%
echo.
echo   - Ping mislukt            : IP-adres of netwerk klopt niet.
echo.

:end
pause
endlocal
