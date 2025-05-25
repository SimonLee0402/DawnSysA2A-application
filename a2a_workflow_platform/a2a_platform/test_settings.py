"""
测试专用的Django设置文件
"""

from .settings import *

# 使用SQLite进行测试
DATABASES = {
    'default': {
        'ENGINE': 'django.db.backends.sqlite3',
        'NAME': BASE_DIR / 'test_db.sqlite3',
    }
}

# 关闭DEBUG模式
DEBUG = False

# 使用简单密码哈希以加速测试
PASSWORD_HASHERS = [
    'django.contrib.auth.hashers.MD5PasswordHasher',
]

# 使用简单的测试密钥
SECRET_KEY = 'django-insecure-test-key'

# 测试媒体文件路径
MEDIA_ROOT = BASE_DIR / 'test_media'

# Fernet密钥（用于AgentCredential加密测试）
AGENT_CREDENTIALS_SECRET = 'YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXoxMjM0NTY='  # 测试用固定密钥 