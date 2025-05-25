@echo off
chcp 65001 > nul
cd /d %~dp0\a2a_workflow_platform
python manage.py runserver 