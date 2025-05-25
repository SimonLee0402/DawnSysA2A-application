from django.urls import path
from rest_framework.authtoken.views import obtain_auth_token
from . import views

urlpatterns = [
    # 认证相关
    path('login/', obtain_auth_token, name='api_token_auth'),
    path('register/', views.UserRegistrationView.as_view(), name='register'),
    path('me/', views.UserProfileView.as_view(), name='user_profile'),
    path('current/', views.CurrentUserView.as_view(), name='current_user'),
    path('logout/', views.LogoutView.as_view(), name='logout'),
    
    # 用户管理（需要管理员权限）
    path('', views.UserListView.as_view(), name='user_list'),
    path('<int:pk>/', views.UserDetailView.as_view(), name='user_detail'),
] 