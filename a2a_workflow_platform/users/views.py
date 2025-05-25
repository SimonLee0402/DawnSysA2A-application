from django.shortcuts import render
from rest_framework import generics, permissions, status
from rest_framework.views import APIView
from rest_framework.response import Response
from rest_framework.authtoken.models import Token
from django.contrib.auth import logout, authenticate, login
from django.views.decorators.csrf import ensure_csrf_cookie
from django.utils.decorators import method_decorator

from .models import User
from .serializers import UserSerializer, UserRegistrationSerializer

class UserRegistrationView(generics.CreateAPIView):
    """
    用户注册视图
    """
    permission_classes = [permissions.AllowAny]
    serializer_class = UserRegistrationSerializer
    
    @method_decorator(ensure_csrf_cookie)
    def get(self, request, *args, **kwargs):
        # 提供一个GET方法来设置CSRF Cookie
        return Response({"detail": "Registration form - CSRF cookie set"})
    
    def post(self, request, *args, **kwargs):
        serializer = self.get_serializer(data=request.data)
        if serializer.is_valid():
            user = serializer.save()
            # 自动登录用户
            login(request, user)
            return Response({
                'success': True,
                'user_id': user.id,
                'username': user.username,
                'email': user.email,
                'user_type': user.user_type
            }, status=status.HTTP_201_CREATED)
        return Response(serializer.errors, status=status.HTTP_400_BAD_REQUEST)


class UserProfileView(generics.RetrieveUpdateAPIView):
    """
    获取和更新当前用户信息
    """
    serializer_class = UserSerializer
    permission_classes = [permissions.IsAuthenticated]
    
    def get_object(self):
        return self.request.user


class CurrentUserView(APIView):
    """
    获取当前登录用户信息
    """
    permission_classes = [permissions.AllowAny]  # 允许所有用户访问
    
    def get(self, request):
        if request.user.is_authenticated:
            serializer = UserSerializer(request.user)
            return Response(serializer.data)
        else:
            return Response({
                "authenticated": False,
                "detail": "未登录"
            }, status=status.HTTP_200_OK)  # 返回200而不是403


class LogoutView(APIView):
    """
    用户注销
    """
    permission_classes = [permissions.IsAuthenticated]
    
    def post(self, request):
        logout(request)
        return Response({"detail": "Successfully logged out."}, status=status.HTTP_200_OK)


class UserListView(generics.ListAPIView):
    """
    用户列表，仅管理员可访问
    """
    queryset = User.objects.all()
    serializer_class = UserSerializer
    permission_classes = [permissions.IsAdminUser]
    
    def get_queryset(self):
        # 可以添加过滤逻辑
        queryset = User.objects.all()
        user_type = self.request.query_params.get('user_type', None)
        if user_type:
            queryset = queryset.filter(user_type=user_type)
        return queryset


class UserDetailView(generics.RetrieveUpdateDestroyAPIView):
    """
    单个用户的详细信息，仅管理员可访问
    """
    queryset = User.objects.all()
    serializer_class = UserSerializer
    permission_classes = [permissions.IsAdminUser]
