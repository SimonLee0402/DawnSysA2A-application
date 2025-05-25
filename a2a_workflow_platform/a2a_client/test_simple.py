"""
简单的功能测试，不需要数据库
"""

import unittest
from django.test import SimpleTestCase
from cryptography.fernet import Fernet
import base64

# 测试密钥
TEST_KEY = base64.urlsafe_b64encode(b'abcdefghijklmnopqrstuvwxyz123456')

class SimpleEncryptionTest(SimpleTestCase):
    """测试加密功能"""
    
    def test_fernet_encryption(self):
        """测试Fernet加密解密功能"""
        key = TEST_KEY
        fernet = Fernet(key)
        
        # 测试数据
        message = "sk-test-api-key"
        
        # 加密
        encrypted = fernet.encrypt(message.encode())
        
        # 确保加密后的数据与原始数据不同
        self.assertNotEqual(encrypted.decode(), message)
        
        # 解密
        decrypted = fernet.decrypt(encrypted).decode()
        
        # 确保解密后的数据与原始数据相同
        self.assertEqual(decrypted, message)

class SimpleModelStructureTest(SimpleTestCase):
    """测试模型结构"""
    
    def test_model_fields(self):
        """测试模型字段定义"""
        from a2a_client.models import Agent, AgentCredential
        
        # 检查Agent模型的字段
        agent_fields = [f.name for f in Agent._meta.fields]
        self.assertIn('name', agent_fields)
        self.assertIn('description', agent_fields)
        self.assertIn('agent_type', agent_fields)
        self.assertIn('model_name', agent_fields)
        self.assertIn('is_active', agent_fields)
        self.assertIn('created_at', agent_fields)
        self.assertIn('updated_at', agent_fields)
        self.assertIn('owner', agent_fields)
        
        # 检查AgentCredential模型的字段
        credential_fields = [f.name for f in AgentCredential._meta.fields]
        self.assertIn('agent', credential_fields)
        self.assertIn('api_key', credential_fields)
        self.assertIn('api_endpoint', credential_fields)
        self.assertIn('additional_params', credential_fields)
        self.assertIn('created_at', credential_fields)
        self.assertIn('updated_at', credential_fields)

class SimpleSerializerTest(SimpleTestCase):
    """测试序列化器结构"""
    
    def test_serializer_fields(self):
        """测试序列化器字段定义"""
        from a2a_client.serializers import AgentSerializer, AgentCredentialSerializer, AgentCreateSerializer
        
        # 检查AgentSerializer的字段
        agent_fields = AgentSerializer().get_fields()
        self.assertIn('name', agent_fields)
        self.assertIn('description', agent_fields)
        self.assertIn('agent_type', agent_fields)
        self.assertIn('model_name', agent_fields)
        self.assertIn('is_active', agent_fields)
        self.assertIn('credential', agent_fields)
        
        # 检查AgentCredentialSerializer的字段
        credential_fields = AgentCredentialSerializer().get_fields()
        self.assertIn('id', credential_fields)
        self.assertIn('api_endpoint', credential_fields)
        self.assertIn('additional_params', credential_fields)
        
        # 检查AgentCreateSerializer的字段
        create_fields = AgentCreateSerializer().get_fields()
        self.assertIn('name', create_fields)
        self.assertIn('api_key', create_fields)
        self.assertIn('api_endpoint', create_fields)
        self.assertIn('additional_params', create_fields) 