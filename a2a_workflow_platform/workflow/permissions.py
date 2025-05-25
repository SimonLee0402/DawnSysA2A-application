from rest_framework import permissions

class IsWorkflowOwnerOrReadOnly(permissions.BasePermission):
    """
    自定义权限类，只允许工作流的创建者编辑它
    """
    
    def has_object_permission(self, request, view, obj):
        # 读取权限允许任何请求
        if request.method in permissions.SAFE_METHODS:
            return True
            
        # 写入权限只允许创建者或管理员
        return obj.created_by == request.user or request.user.is_staff


class IsInstanceOwnerOrReadOnly(permissions.BasePermission):
    """
    自定义权限类，只允许工作流实例的启动者编辑它
    """
    
    def has_object_permission(self, request, view, obj):
        # 读取权限允许任何请求
        if request.method in permissions.SAFE_METHODS:
            return True
            
        # 写入权限只允许启动者或管理员
        return obj.started_by == request.user or request.user.is_staff 