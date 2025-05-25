from django.test import TestCase
from django.test import override_settings
from django.contrib.auth import get_user_model
from rest_framework.test import APITestCase, APIClient
from rest_framework import status
from django.urls import reverse
from .models import Agent, AgentCredential
import uuid
import base64
from cryptography.fernet import Fernet

User = get_user_model()

# 生成测试用的Fernet密钥
TEST_KEY = base64.urlsafe_b64encode(b'abcdefghijklmnopqrstuvwxyz123456')

@override_settings(AGENT_CREDENTIALS_SECRET=TEST_KEY.decode())
class AgentModelTest(TestCase):
    """测试Agent模型"""
    
    def setUp(self):
        # 创建测试用户，使用create()方法而不是create_user()
        self.user = User.objects.create(
            username='testuser',
            email='test@example.com',
            is_active=True
        )
        self.user.set_password('password123')
        self.user.save()
        
        # 创建Agent实例
        self.agent = Agent.objects.create(
            name='测试Agent',
            description='这是一个测试Agent',
            agent_type='gpt-3.5',
            model_name='gpt-3.5-turbo',
            owner=self.user
        )
        
        # 创建AgentCredential实例
        self.credential = AgentCredential.objects.create(
            agent=self.agent,
            api_key='sk-test-key',
            api_endpoint='https://api.example.com/v1/chat/completions'
        )
    
    def test_agent_creation(self):
        """测试Agent实例创建"""
        self.assertEqual(self.agent.name, '测试Agent')
        self.assertEqual(self.agent.agent_type, 'gpt-3.5')
        self.assertEqual(self.agent.owner, self.user)
        self.assertTrue(self.agent.is_active)
    
    def test_agent_credential_creation(self):
        """测试AgentCredential实例创建"""
        self.assertEqual(self.credential.agent, self.agent)
        self.assertEqual(self.credential.api_endpoint, 'https://api.example.com/v1/chat/completions')
        
        # 检查密钥是否被正确加密
        self.assertTrue(self.credential.api_key.startswith('encrypted:'))
        
        # 检查是否可以正确解密
        decrypted_key = self.credential.get_api_key()
        self.assertEqual(decrypted_key, 'sk-test-key')


@override_settings(AGENT_CREDENTIALS_SECRET=TEST_KEY.decode())
class AgentAPITest(APITestCase):
    """测试Agent API"""
    
    def setUp(self):
        # 创建测试用户，使用create()方法而不是create_user()
        self.user = User.objects.create(
            username='testuser',
            email='test@example.com',
            is_active=True
        )
        self.user.set_password('password123')
        self.user.save()
        
        # 创建另一个用户，用于测试权限
        self.other_user = User.objects.create(
            username='otheruser',
            email='other@example.com',
            is_active=True
        )
        self.other_user.set_password('password123')
        self.other_user.save()
        
        # 设置认证
        self.client = APIClient()
        self.client.force_authenticate(user=self.user)
        
        # 创建Agent实例
        self.agent = Agent.objects.create(
            name='测试Agent',
            description='这是一个测试Agent',
            agent_type='gpt-3.5',
            model_name='gpt-3.5-turbo',
            owner=self.user
        )
        
        # 创建AgentCredential实例
        self.credential = AgentCredential.objects.create(
            agent=self.agent,
            api_key='sk-test-key',
            api_endpoint='https://api.example.com/v1/chat/completions'
        )
        
        # API URL
        self.agents_url = reverse('a2a_client:agent-list')
        self.agent_detail_url = reverse('a2a_client:agent-detail', kwargs={'pk': self.agent.id})
    
    def test_list_agents(self):
        """测试列出用户的所有Agent"""
        response = self.client.get(self.agents_url)
        self.assertEqual(response.status_code, status.HTTP_200_OK)
        self.assertIn('results', response.data)
        
        # 更简单的检查，仅确保响应成功且有结果
        self.assertTrue(len(response.data['results']) > 0)
        self.assertEqual(response.data['results'][0]['name'], '测试Agent')
    
    def test_create_agent(self):
        """测试创建新Agent"""
        data = {
            'name': '新Agent',
            'description': '这是一个新的测试Agent',
            'agent_type': 'gpt-4',
            'model_name': 'gpt-4-turbo',
            'is_active': True,
            'api_key': 'sk-new-test-key',
            'api_endpoint': 'https://api.example.com/v1/chat/completions'
        }
        
        response = self.client.post(self.agents_url, data, format='json')
        self.assertEqual(response.status_code, status.HTTP_201_CREATED)
        self.assertEqual(Agent.objects.count(), 2)
        
        # 检查是否创建了一个名为"新Agent"的Agent
        self.assertTrue(Agent.objects.filter(name='新Agent').exists())
        
        # 获取新创建的Agent
        new_agent = Agent.objects.get(name='新Agent')
        
        # 检查凭证是否被创建
        self.assertTrue(hasattr(new_agent, 'credential'))
        
        # API响应中不应该包含API密钥
        self.assertNotIn('api_key', response.data)
    
    def test_retrieve_agent(self):
        """测试获取特定Agent"""
        response = self.client.get(self.agent_detail_url)
        self.assertEqual(response.status_code, status.HTTP_200_OK)
        self.assertEqual(response.data['name'], '测试Agent')
        self.assertEqual(response.data['agent_type'], 'gpt-3.5')
        
        # 检查响应中包含了凭证信息但不包含API密钥
        self.assertIn('credential', response.data)
        self.assertNotIn('api_key', response.data['credential'])
    
    def test_update_agent(self):
        """测试更新Agent"""
        data = {
            'name': '更新后的Agent',
            'description': '这是更新后的描述'
        }
        
        response = self.client.patch(self.agent_detail_url, data, format='json')
        self.assertEqual(response.status_code, status.HTTP_200_OK)
        self.agent.refresh_from_db()
        self.assertEqual(self.agent.name, '更新后的Agent')
        self.assertEqual(self.agent.description, '这是更新后的描述')
    
    def test_permission_other_user(self):
        """测试其他用户的权限"""
        # 切换到其他用户
        self.client.force_authenticate(user=self.other_user)
        
        # 尝试获取详情
        response = self.client.get(self.agent_detail_url)
        self.assertEqual(response.status_code, status.HTTP_404_NOT_FOUND)
        
        # 尝试更新
        data = {'name': '非法更新'}
        response = self.client.patch(self.agent_detail_url, data, format='json')
        self.assertEqual(response.status_code, status.HTTP_404_NOT_FOUND)
        
        # 尝试删除
        response = self.client.delete(self.agent_detail_url)
        self.assertEqual(response.status_code, status.HTTP_404_NOT_FOUND)
        
        # 检查Agent是否仍然存在
        self.assertTrue(Agent.objects.filter(id=self.agent.id).exists())
