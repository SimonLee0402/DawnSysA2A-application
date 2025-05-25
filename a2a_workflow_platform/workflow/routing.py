from django.urls import re_path
from . import consumers

websocket_urlpatterns = [
    re_path(r'ws/workflow/instance/(?P<instance_id>[^/]+)/$', consumers.WorkflowInstanceConsumer.as_asgi()),
] 