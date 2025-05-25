from django.urls import path
from . import views

app_name = 'dashboard'

urlpatterns = [
    path('', views.DashboardStatsView.as_view(), name='stats'), # 指向 DashboardStatsView
    # 稍后将在这里添加 dashboard API 的具体路由
    # 例如: path('', views.DashboardStatsView.as_view(), name='stats'),
] 