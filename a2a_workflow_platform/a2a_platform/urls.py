"""
主URL配置
"""
from django.contrib import admin
from django.urls import path, include
from django.conf import settings
from django.conf.urls.static import static
from django.contrib.auth import views as auth_views
from frontend.views import VueAppView, HomeView
from users.views import UserRegistrationView
from django.http import JsonResponse, HttpResponse
from django.urls import get_resolver
from .views import csrf  # 导入CSRF视图
from django.views.generic import TemplateView
from django.views.static import serve
import os

def list_urls(request):
    """临时视图，用于列出所有URL"""
    urls = []
    
    def collect_urls(resolver, prefix=''):
        for pattern in resolver.url_patterns:
            if hasattr(pattern, 'url_patterns'):
                # 这是一个包含其他URL的pattern
                new_prefix = prefix + str(pattern.pattern)
                collect_urls(pattern, new_prefix)
            else:
                # 这是一个叶子节点，表示一个URL
                pattern_str = str(pattern.pattern)
                if hasattr(pattern, 'name') and pattern.name:
                    name = pattern.name
                else:
                    name = '(no name)'
                urls.append({
                    'url': prefix + pattern_str,
                    'name': name,
                })
    
    collect_urls(get_resolver())
    return JsonResponse(urls, safe=False)

# 自定义JavaScript文件服务函数
def serve_js_file(request, filename):
    """为特定API JS文件提供服务的视图"""
    js_content = """
    // 由Django动态生成的JS文件
    console.log('API: 加载 %s.js');
    
    // 导出一个空对象，防止导入错误
    export default {};
    """ % filename
    
    return HttpResponse(js_content, content_type='application/javascript')

# API中间件服务函数
def serve_api_middleware(request):
    """为API中间件JS文件提供服务的视图"""
    middleware_content = """
    /**
     * API中间件 - 由Django动态生成
     */
    
    import axios from 'axios';
    
    // 设置请求拦截器
    axios.interceptors.request.use(function (config) {
      // 在发送请求之前做些什么
      console.log('API中间件: 请求拦截', config.url);
      return config;
    }, function (error) {
      // 对请求错误做些什么
      console.error('API中间件: 请求错误', error);
      return Promise.reject(error);
    });
    
    // 设置响应拦截器
    axios.interceptors.response.use(function (response) {
      // 对响应数据做点什么
      console.log('API中间件: 响应成功', response.config.url);
      return response;
    }, function (error) {
      // 对响应错误做点什么
      console.error('API中间件: 响应错误', error.config ? error.config.url : '未知URL', error);
      return Promise.reject(error);
    });
    
    export default {
      name: 'api-middleware',
      version: '1.0.0'
    };
    """
    
    return HttpResponse(middleware_content, content_type='application/javascript')

urlpatterns = [
    path('admin/', admin.site.urls),
    
    # 前端URL - 使用Vue应用作为主入口
    path('', VueAppView.as_view(), name='home'),  # 根路径直接使用Vue应用
    path('app/', VueAppView.as_view(), name='vue_app'),  # 保留专用Vue路径
    
    # 登录和注册的前端Vue路由 - 确保这些路径返回Vue应用
    path('login', VueAppView.as_view(), name='vue_login'),  # 不带斜杠
    path('login/', VueAppView.as_view(), name='vue_login_slash'),  # 带斜杠
    path('register', VueAppView.as_view(), name='vue_register'),  # 不带斜杠
    path('register/', VueAppView.as_view(), name='vue_register_slash'),  # 带斜杠
    
    # 包含前端其他URLs
    path('', include('frontend.urls')),
    
    # CSRF Cookie URL
    path('csrf/', csrf, name='csrf'),
    path('api/csrf/', csrf, name='api_csrf'),
    
    # 认证URL - 确保这些是API端点，不是页面
    path('accounts/login/', auth_views.LoginView.as_view(), name='login'),
    path('accounts/logout/', auth_views.LogoutView.as_view(), name='logout'),
    path('register/', UserRegistrationView.as_view(), name='register'),  # 注册URL
    
    # 处理特定API JS文件请求 - 确保传递正确的filename参数
    path('api/auth-repair.js', lambda request: serve_js_file(request, 'auth-repair'), name='serve_auth_repair'),
    path('api/auth-repair', lambda request: serve_js_file(request, 'auth-repair'), name='serve_auth_repair_no_ext'),
    path('api/index.js', lambda request: serve_js_file(request, 'index'), name='serve_index_js'),
    path('api/index', lambda request: serve_js_file(request, 'index'), name='serve_index_js_no_ext'),
    path('api/axios-config.js', lambda request: serve_js_file(request, 'axios-config'), name='serve_axios_config'),
    path('api/axios-config', lambda request: serve_js_file(request, 'axios-config'), name='serve_axios_config_no_ext'),
    path('api/api-middleware.js', serve_api_middleware, name='serve_api_middleware'),
    path('api/api-middleware', serve_api_middleware, name='serve_api_middleware_no_ext'),
    
    # API端点
    path('api/agents/', include('agents.api.urls')),
    path('api/workflows/', include('workflow.api.urls')),
    path('api/a2a-client/', include('a2a_client.urls')),
    path('api/users/', include('users.urls')),
    path('api/dashboard/', include('dashboard.urls')),
    path('api/knowledgebase/', include('knowledgebase.urls')), # 取消注释
    path('api-auth/', include('rest_framework.urls')),  # 添加DRF认证URLs
    
    # 临时URL列表查看端点
    path('debug/urls/', list_urls, name='list_urls'),
    # path('api/admin/', admin.site.urls), # 注释掉此行以避免 admin 命名空间冲突
    
    # 通配符路由，放在最后作为后备
    # path('api/<str:filename>.js', serve_js_file, name='serve_js_file'),
    # path('api/<str:filename>', serve_js_file, name='serve_js_file_without_ext'),
]

if settings.DEBUG:
    urlpatterns += static(settings.MEDIA_URL, document_root=settings.MEDIA_ROOT)
    urlpatterns += static(settings.STATIC_URL, document_root=settings.STATIC_ROOT)
