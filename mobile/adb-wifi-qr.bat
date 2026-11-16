@echo off
setlocal EnableDelayedExpansion
rem Pareia Android 11+ via "Depuracao por Wi-Fi > Parear dispositivo com QR code".
rem Requer adb (platform-tools) e node/npx no PATH; PC e celular na mesma rede.

chcp 65001 >nul
set "NAME=studio-livecmd%RANDOM%"
set "PASS=%RANDOM%%RANDOM%"

adb start-server >nul
echo.
echo No celular: Opcoes do desenvolvedor ^> Depuracao por Wi-Fi ^> Parear dispositivo com QR code
echo.
call npx -y qrcode-terminal "WIFI:T:ADB;S:%NAME%;P:%PASS%;;"
echo.
echo Aguardando leitura do QR (Ctrl+C cancela)...

set /a TRIES=0
:wait
set "ADDR="
for /f "tokens=1,3" %%a in ('adb mdns services 2^>nul ^| findstr /c:"%NAME%"') do set "ADDR=%%b"
if defined ADDR goto pair
set /a TRIES+=1
if %TRIES% geq 60 (echo Tempo esgotado. & exit /b 1)
timeout /t 2 /nobreak >nul
goto wait

:pair
echo Pareando com %ADDR%...
adb pair %ADDR% %PASS% || exit /b 1

rem adb conecta sozinho apos parear; espera ate aparecer.
set /a TRIES=0
:conn
adb devices | findstr /r /c:"_adb-tls-connect" /c:":[0-9]*	device" >nul && goto done
set /a TRIES+=1
if %TRIES% geq 15 (echo Pareado, mas sem conexao. Rode: adb connect IP:PORTA da tela "Depuracao por Wi-Fi". & exit /b 1)
timeout /t 2 /nobreak >nul
goto conn

:done
adb devices
echo Conectado.
