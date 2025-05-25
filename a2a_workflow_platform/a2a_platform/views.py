from django.http import JsonResponse
from django.views.decorators.csrf import ensure_csrf_cookie

@ensure_csrf_cookie
def csrf(request):
    """
    设置CSRF Cookie的视图
    当前端需要获取CSRF令牌时，可以调用此视图
    """
    return JsonResponse({'detail': 'CSRF cookie set'}) 