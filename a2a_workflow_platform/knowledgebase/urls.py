from django.urls import path, include
from rest_framework.routers import DefaultRouter

from .views import KnowledgeBaseViewSet # DocumentViewSet不再由router注册

router = DefaultRouter()
router.register(r'knowledgebases', KnowledgeBaseViewSet, basename='knowledgebase')
# router.register(r'documents', DocumentViewSet, basename='document') # 移除此行

app_name = 'knowledgebase'

urlpatterns = [
    path('', include(router.urls)),
] 