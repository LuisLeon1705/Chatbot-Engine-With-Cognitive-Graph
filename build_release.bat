@echo off
rem ==============================================================================
rem Chats - Script de Compilación en Modo Release
rem Compila el motor Rust (CLI) y la aplicación de escritorio (Tauri v2 + React)
rem ==============================================================================
chcp 65001 >nul
setlocal enabledelayedexpansion
cd /d "%~dp0"
title Chats - Compilación Release

set "APP_EXE=target\release\chats-app.exe"
set "CLI_EXE=target\release\chats.exe"

:menu
cls
echo.
echo   ========================================================================
echo     CHATS: Cognitive GraphRAG Engine  -  Compilación Release
echo   ========================================================================
echo.
echo     1. Compilar TODO (Motor CLI + App de Escritorio Tauri) [Recomendado]
echo     2. Compilar únicamente el Motor CLI (cargo build --release -p chats-cli)
echo     3. Compilar únicamente la App Tauri (npx tauri build --no-bundle)
echo     4. Ejecutar Suite de Tests y Verificación de Tipos (cargo test + tsc)
echo     0. Salir
echo.
set "CHOICE="
set /p "CHOICE=  Selecciona una opción [1-4, 0]: "

if "%CHOICE%"=="1" goto :build_all
if "%CHOICE%"=="2" goto :build_cli
if "%CHOICE%"=="3" goto :build_app
if "%CHOICE%"=="4" goto :run_tests
if "%CHOICE%"=="0" goto :exit
goto :menu

rem ------------------------------------------------------------------------------
:check_running
rem Cierra la app si está corriendo para evitar el error "Access is denied"
tasklist /FI "IMAGENAME eq chats-app.exe" 2>NUL | find /I /N "chats-app.exe">NUL
if "%ERRORLEVEL%"=="0" (
    echo.
    echo   [AVISO] 'chats-app.exe' está en ejecución.
    echo   Cerrándolo para evitar error de acceso denegado durante el enlace...
    taskkill /IM chats-app.exe /F >nul 2>&1
    timeout /t 1 /nobreak >nul
)
exit /b 0

rem ------------------------------------------------------------------------------
:check_prereqs
where cargo >nul 2>&1
if errorlevel 1 (
    echo.
    echo   [ERROR] Rust/Cargo no está instalado o no se encuentra en el PATH.
    pause
    exit /b 1
)
where npm >nul 2>&1
if errorlevel 1 (
    echo.
    echo   [ERROR] Node.js/npm no está instalado o no se encuentra en el PATH.
    pause
    exit /b 1
)
exit /b 0

rem ------------------------------------------------------------------------------
:build_cli
call :check_prereqs || goto :menu
echo.
echo   ========================================================================
echo   [1/1] Compilando Chats CLI en modo Release...
echo   ========================================================================
cargo build --release -p chats-cli
if errorlevel 1 (
    echo.
    echo   [ERROR] La compilación del CLI falló. Revisa los errores anteriores.
    pause
    goto :menu
)
echo.
echo   [OK] CLI compilado con éxito en: %CLI_EXE%
pause
goto :menu

rem ------------------------------------------------------------------------------
:build_app
call :check_prereqs || goto :menu
call :check_running
echo.
echo   ========================================================================
echo   [1/1] Compilando Chats Desktop App (Tauri v2 + Frontend React)...
echo   ========================================================================
if not exist node_modules (
    echo   Instalando dependencias de frontend (npm install)...
    call npm install
    if errorlevel 1 (
        echo   [ERROR] npm install falló.
        pause
        goto :menu
    )
)
echo   Ejecutando npx tauri build --no-bundle...
call npx tauri build --no-bundle
if errorlevel 1 (
    echo.
    echo   [ERROR] La compilación de la app Tauri falló.
    pause
    goto :menu
)
echo.
echo   [OK] App de escritorio compilada con éxito en: %APP_EXE%
pause
goto :menu

rem ------------------------------------------------------------------------------
:build_all
call :check_prereqs || goto :menu
call :check_running
echo.
echo   ========================================================================
echo   Paso 1: Compilando Workspace y CLI en modo Release...
echo   ========================================================================
cargo build --release -p chats-cli
if errorlevel 1 (
    echo.
    echo   [ERROR] Falló la compilación del CLI.
    pause
    goto :menu
)

echo.
echo   ========================================================================
echo   Paso 2: Compilando Frontend y Binario Tauri (chats-app.exe)...
echo   ========================================================================
if not exist node_modules (
    echo   Instalando dependencias de frontend...
    call npm install
    if errorlevel 1 (
        echo   [ERROR] npm install falló.
        pause
        goto :menu
    )
)
call npx tauri build --no-bundle
if errorlevel 1 (
    echo.
    echo   [ERROR] Falló la compilación de la app Tauri.
    pause
    goto :menu
)

echo.
echo   ========================================================================
echo   ¡COMPILACIÓN RELEASE COMPLETADA CON ÉXITO!
echo   ========================================================================
echo   - CLI:  %CLI_EXE%
echo   - App:  %APP_EXE%
echo   ========================================================================
echo.
pause
goto :menu

rem ------------------------------------------------------------------------------
:run_tests
call :check_prereqs || goto :menu
echo.
echo   ========================================================================
echo   Verificando Tipos de TypeScript (tsc --noEmit)...
echo   ========================================================================
call npx tsc --noEmit -p .
if errorlevel 1 (
    echo   [ERROR] Errores de tipo en TypeScript detectados.
    pause
    goto :menu
)
echo   [OK] TypeScript sin errores.

echo.
echo   ========================================================================
echo   Ejecutando Suite de Tests en Rust (cargo test --workspace)...
echo   ========================================================================
cargo test --workspace
if errorlevel 1 (
    echo   [ERROR] Fallaron tests unitarios o de integración.
    pause
    goto :menu
)
echo.
echo   [OK] Todos los tests pasaron exitosamente.
pause
goto :menu

:exit
endlocal
exit /b 0
