@echo off
rem Chats launcher: menu to chat, look at the memory and check the models.
rem The texts follow the language chosen with "chats lang" (or option L).
chcp 65001 >nul
setlocal
cd /d "%~dp0"
title Chats

set "CHATS=target\release\chats.exe"
set "APP=target\release\chats-app.exe"
if not exist "%CHATS%" call :build || goto :end

set "WORLD=arknights/terra"
set "SPEAKER=Kal'tsit"
set "WITH=Amiya,Doctor"
set "ME="

:menu
call :texts
if "%ME%"=="" set "ME=%T_USER%"
cls
echo.
echo   ============================================
echo     CHATS  -  %T_TITLE%
echo   ============================================
echo.
echo     %T_WORLD%: %WORLD%    %T_SPEAKS%: %SPEAKER%    %T_WITH%: %WITH%    %T_YOU%: %ME%
echo.
echo     A. %T_A%
echo.
echo     1. %T_1%
echo     2. %T_2%
echo     3. %T_3%
echo     4. %T_4%
echo     5. %T_5%
echo     6. %T_6%
echo     7. %T_7%
echo     8. %T_8%
echo     9. %T_9%
echo     L. %T_L%
echo     0. %T_0%
echo.
set "OP="
set /p "OP=  %T_OPTION%: "
if /i "%OP%"=="A" goto :app
if /i "%OP%"=="L" goto :language
if "%OP%"=="1" goto :chat
if "%OP%"=="2" goto :context
if "%OP%"=="3" goto :learned
if "%OP%"=="4" goto :change
if "%OP%"=="5" goto :worlds
if "%OP%"=="6" goto :models
if "%OP%"=="7" goto :seed
if "%OP%"=="8" goto :rebuild
if "%OP%"=="9" goto :stop
if "%OP%"=="0" goto :end
goto :menu

:chat
cls
echo  %T_CHAT_HELP%
echo    /speak ^<name^>     %T_CHAT_SPEAK%
echo    /with A,B          %T_CHAT_WITH%
echo    /memory            %T_CHAT_MEMORY%
echo    /image [scene]     %T_CHAT_IMAGE%
echo    /exit, Ctrl+C      %T_CHAT_EXIT%
echo.
if "%WITH%"=="" (
  "%CHATS%" chat %WORLD% "%SPEAKER%" --as "%ME%"
) else (
  "%CHATS%" chat %WORLD% "%SPEAKER%" --with "%WITH%" --as "%ME%"
)
echo.
pause
goto :menu

:context
echo.
set "MSG="
set /p "MSG=  %T_TEST_MESSAGE%: "
if "%MSG%"=="" goto :menu
if "%WITH%"=="" (
  "%CHATS%" context %WORLD% "%SPEAKER%" "%MSG%" --as "%ME%"
) else (
  "%CHATS%" context %WORLD% "%SPEAKER%" "%MSG%" --with "%WITH%" --as "%ME%"
)
echo.
pause
goto :menu

:learned
"%CHATS%" memory %WORLD% "%SPEAKER%" 20
echo.
pause
goto :menu

:change
echo.
echo  %T_CHANGE_HELP%
set /p "WORLD=  %T_WORLD% [%WORLD%]: "
set /p "SPEAKER=  %T_SPEAKS% [%SPEAKER%]: "
set /p "WITH=  %T_WITH% [%WITH%]: "
set /p "ME=  %T_YOU% [%ME%]: "
if "%WITH%"=="-" set "WITH="
goto :menu

:worlds
"%CHATS%" worlds
echo.
pause
goto :menu

:models
"%CHATS%" models
echo.
pause
goto :menu

:seed
echo.
set "TOML="
set /p "TOML=  %T_SEED_FILE%: "
if not "%TOML%"=="" "%CHATS%" seed "%TOML%"
echo.
pause
goto :menu

:language
if "%LANGUAGE%"=="es" ("%CHATS%" lang en) else ("%CHATS%" lang es)
goto :menu

:rebuild
call :build
call :buildapp
pause
goto :menu

:app
if not exist "%APP%" call :buildapp || goto :menu
start "" "%APP%"
goto :menu

:stop
echo.
echo  %T_STOP_WARN%
set "OK="
set /p "OK=  %T_CONTINUE%: "
if /i "%OK%"=="y" set "OK=s"
if /i "%OK%"=="s" (
  taskkill /IM llama-server.exe /F >nul 2>&1 && echo  %T_STOPPED% || echo  %T_NONE_RUNNING%
  taskkill /IM sd-cli.exe /F >nul 2>&1
)
pause
goto :menu

:build
echo  Building Chats (the first time takes a couple of minutes)...
cargo build --release -p chats-cli
if errorlevel 1 (
  echo  The build failed.
  pause
  exit /b 1
)
exit /b 0

:buildapp
echo  Building the app (the first time takes a few minutes)...
if not exist node_modules call npm install
call npx tauri build --no-bundle
if errorlevel 1 (
  echo  The app build failed.
  pause
  exit /b 1
)
exit /b 0

rem Menu texts in the current language ("chats lang" prints en or es).
:texts
set "LANGUAGE=en"
for /f "usebackq delims=" %%L in (`"%CHATS%" lang 2^>nul`) do set "LANGUAGE=%%L"
if "%LANGUAGE%"=="es" goto :texts_es
set "T_TITLE=graph memory for characters"
set "T_WORLD=World"
set "T_SPEAKS=Speaker"
set "T_WITH=With"
set "T_YOU=You"
set "T_USER=User"
set "T_A=Open the app (chats, groups and memory manager)"
set "T_1=Chat in the console (loads the model on the GPU)"
set "T_2=See the context the character gets (no models)"
set "T_3=See what the character has learned (no models)"
set "T_4=Change world / character / companions / your name"
set "T_5=List worlds and characters"
set "T_6=List detected models"
set "T_7=Seed a world from a .toml"
set "T_8=Rebuild (console and app)"
set "T_9=Stop models left running"
set "T_L=Cambiar a español"
set "T_0=Exit"
set "T_OPTION=Option"
set "T_CHAT_HELP=Commands inside the chat:"
set "T_CHAT_SPEAK=changes who answers (must be in the scene)"
set "T_CHAT_WITH=changes who is in the scene"
set "T_CHAT_MEMORY=shows the context of the last turn"
set "T_CHAT_IMAGE=portrait of the speaker (needs an image model)"
set "T_CHAT_EXIT=saves the memory and frees the GPU"
set "T_TEST_MESSAGE=Test message"
set "T_CHANGE_HELP=(Enter keeps the current value. For companions, type - for nobody.)"
set "T_SEED_FILE=.toml file (e.g. seeds\terra.toml)"
set "T_STOP_WARN=This closes ALL llama-server and sd-cli processes on this computer."
set "T_CONTINUE=Continue? (y/n)"
set "T_STOPPED=llama-server stopped."
set "T_NONE_RUNNING=There was no llama-server running."
exit /b 0

:texts_es
set "T_TITLE=memoria en grafo para personajes"
set "T_WORLD=Mundo"
set "T_SPEAKS=Habla"
set "T_WITH=Con"
set "T_YOU=Tú"
set "T_USER=Usuario"
set "T_A=Abrir la app (chats, grupos y gestor de memoria)"
set "T_1=Chatear en la consola (carga el modelo en la GPU)"
set "T_2=Ver el contexto que recibe el personaje (sin modelos)"
set "T_3=Ver lo que ha aprendido el personaje (sin modelos)"
set "T_4=Cambiar mundo / personaje / acompañantes / tu nombre"
set "T_5=Ver mundos y personajes"
set "T_6=Ver modelos detectados"
set "T_7=Sembrar un mundo desde un .toml"
set "T_8=Recompilar (consola y app)"
set "T_9=Detener modelos que se hayan quedado abiertos"
set "T_L=Switch to English"
set "T_0=Salir"
set "T_OPTION=Opción"
set "T_CHAT_HELP=Comandos dentro del chat:"
set "T_CHAT_SPEAK=cambia quién responde (debe estar en la escena)"
set "T_CHAT_WITH=cambia quién está en la escena"
set "T_CHAT_MEMORY=muestra el contexto del último turno"
set "T_CHAT_IMAGE=retrato del que habla (necesita modelo de imagen)"
set "T_CHAT_EXIT=guarda la memoria y libera la GPU"
set "T_TEST_MESSAGE=Mensaje de prueba"
set "T_CHANGE_HELP=(Enter deja el valor actual. Para acompañantes, escribe - para nadie.)"
set "T_SEED_FILE=Archivo .toml (p. ej. seeds\terra.toml)"
set "T_STOP_WARN=Esto cierra TODOS los procesos llama-server y sd-cli de este equipo."
set "T_CONTINUE=¿Continuar? (s/n)"
set "T_STOPPED=llama-server detenido."
set "T_NONE_RUNNING=No había ningún llama-server."
exit /b 0

:end
endlocal
