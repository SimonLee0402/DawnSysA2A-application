from rest_framework import serializers
from .models import User, UserProfile

class UserProfileSerializer(serializers.ModelSerializer):
    class Meta:
        model = UserProfile
        fields = '__all__'
        read_only_fields = ['user']

class UserSerializer(serializers.ModelSerializer):
    profile = UserProfileSerializer(read_only=True)
    
    class Meta:
        model = User
        fields = ['id', 'username', 'email', 'first_name', 'last_name', 
                 'user_type', 'company_name', 'channel_name', 'platform', 
                 'is_active', 'is_staff', 'date_joined', 'profile']
        read_only_fields = ['id', 'date_joined', 'is_active']

class UserRegistrationSerializer(serializers.ModelSerializer):
    password = serializers.CharField(write_only=True)
    password_confirm = serializers.CharField(write_only=True)
    
    class Meta:
        model = User
        fields = ['username', 'email', 'password', 'password_confirm', 
                 'first_name', 'last_name', 'user_type', 
                 'company_name', 'channel_name', 'platform']
    
    def validate(self, data):
        # 验证两次密码是否一致
        if data['password'] != data['password_confirm']:
            raise serializers.ValidationError({"password_confirm": "两次密码不一致"})
        return data
    
    def create(self, validated_data):
        # 移除password_confirm字段
        validated_data.pop('password_confirm')
        # 创建用户
        password = validated_data.pop('password')
        user = User(**validated_data)
        user.set_password(password)
        user.save()
        
        # 创建用户资料
        UserProfile.objects.create(user=user)
        
        return user 