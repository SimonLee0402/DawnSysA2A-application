from rest_framework import viewsets, permissions, status, serializers
from rest_framework.response import Response
from rest_framework.decorators import action
from rest_framework.parsers import MultiPartParser, FormParser
from django.shortcuts import get_object_or_404
from django.utils import timezone
from io import BytesIO # Add for handling in-memory file for pypdf
from django.db.models import Q # Import Q object

# Attempt to import pypdf, but don't fail if it's not critical for all ops
try:
    from pypdf import PdfReader
    PYPDF_AVAILABLE = True
except ImportError:
    PYPDF_AVAILABLE = False

# Attempt to import python-docx
try:
    import docx
    PYTHON_DOCX_AVAILABLE = True
except ImportError:
    PYTHON_DOCX_AVAILABLE = False

from .models import KnowledgeBase, Document, VisibilityChoices # Import VisibilityChoices
from .serializers import KnowledgeBaseSerializer, DocumentSerializer

class IsOwnerOrReadOnly(permissions.BasePermission):
    """
    Custom permission to only allow owners of an object to edit it,
    but allow read-only for any authenticated user if the object is public.
    """
    def has_object_permission(self, request, view, obj):
        # Read permissions are allowed for any request,
        # so we'll always allow GET, HEAD or OPTIONS requests.
        if request.method in permissions.SAFE_METHODS:
            if isinstance(obj, KnowledgeBase):
                if obj.visibility == VisibilityChoices.PUBLIC:
                    return True
                return obj.owner == request.user # For private, only owner can read
            return True # For other objects, or if not a KB, default to allowing safe methods

        # Write permissions are only allowed to the owner of the snippet.
        return obj.owner == request.user

class KnowledgeBaseViewSet(viewsets.ModelViewSet):
    """API endpoint for managing Knowledge Bases."""
    serializer_class = KnowledgeBaseSerializer
    permission_classes = [permissions.IsAuthenticated, IsOwnerOrReadOnly] # Use custom permission

    def get_queryset(self):
        user = self.request.user
        if not user.is_authenticated:
            return KnowledgeBase.objects.none() # Should not happen if IsAuthenticated is first
        # Users can see all their own KBs (private or public)
        # Users can also see all public KBs from other users
        return KnowledgeBase.objects.filter(
            Q(owner=user) | Q(visibility=VisibilityChoices.PUBLIC)
        ).distinct().order_by('-updated_at')

    def perform_create(self, serializer):
        serializer.save(owner=self.request.user)

    @action(detail=True, methods=['get'], url_path='documents')
    def list_documents(self, request, pk=None):
        knowledge_base = self.get_object() # get_object will use get_queryset + IsOwnerOrReadOnly
        # No need for explicit owner check here due to IsOwnerOrReadOnly handling object access
        
        documents = Document.objects.filter(knowledge_base=knowledge_base).order_by('-uploaded_at')
        serializer = DocumentSerializer(documents, many=True, context={'request': request})
        return Response(serializer.data)

    @action(detail=True, methods=['post'], url_path='upload_document', parser_classes=[MultiPartParser, FormParser])
    def upload_document_to_kb(self, request, pk=None):
        knowledge_base = self.get_object()
        # Permission to upload to this KB is implicitly handled by IsOwnerOrReadOnly on the KB itself.
        # If only owners can upload, IsOwnerOrReadOnly already covers it for non-SAFE_METHODS.

        file_obj = request.data.get('file')
        doc_file_name = request.data.get('file_name', file_obj.name if file_obj else 'Unnamed Document')
        
        if not file_obj:
            return Response({'detail': 'No file provided.'}, status=status.HTTP_400_BAD_REQUEST)

        doc_file_type = 'unknown'
        if file_obj.name:
            parts = file_obj.name.split('.')
            if len(parts) > 1:
                extension = parts[-1].lower()
                if len(extension) <= 50: 
                    doc_file_type = extension
        
        if doc_file_type == 'unknown' or len(doc_file_type) > 50: 
            if hasattr(file_obj, 'content_type') and file_obj.content_type:
                content_type_short = file_obj.content_type.split('/')[-1]
                if len(content_type_short) <= 50:
                    doc_file_type = content_type_short
        
        if doc_file_type == 'unknown' or len(doc_file_type) > 50:
             doc_file_type = 'bin' 

        # Use DocumentSerializer for creating the document instance
        # Pass knowledge_base instance directly to the serializer context or save call
        document_data = {
            'file_name': doc_file_name,
            'file_type': doc_file_type,
            # 'original_file': file_obj # This is handled by serializer.save if 'original_file' is a writable field
        }
        doc_serializer = DocumentSerializer(data=document_data, context={'request': request})
        
        if doc_serializer.is_valid():
            document = doc_serializer.save(
                knowledge_base=knowledge_base, 
                original_file=file_obj # Explicitly pass file_obj to original_file field of Document model
            )
            self._process_document_content(document, file_obj)
            return Response(DocumentSerializer(document, context={'request': request}).data, status=status.HTTP_201_CREATED)
        return Response(doc_serializer.errors, status=status.HTTP_400_BAD_REQUEST)

    def _process_document_content(self, document, file_obj):
        """
        Processes the uploaded document file to extract text.
        Currently, this is a placeholder. In a real scenario, this would involve:
        - Reading the file content from document.original_file or the passed file_obj.
        - Using libraries like PyPDF2 for PDFs, python-docx for DOCX, etc.
        - Storing the extracted text in document.extracted_text.
        - Handling potential errors during processing.
        - This should ideally be an asynchronous task (e.g., using Celery).
        """
        document.status = Document.ProcessingStatus.PROCESSING
        document.save(update_fields=['status'])

        try:
            extracted_text = f"Unsupported file type: {document.file_type} or error in processing."
            
            if document.file_type == 'txt':
                file_obj.seek(0) # Ensure reading from the beginning
                try:
                    content_bytes = file_obj.read()
                    extracted_text = content_bytes.decode('utf-8')
                except UnicodeDecodeError:
                    # Try with a common alternative encoding if UTF-8 fails
                    try:
                        file_obj.seek(0)
                        content_bytes = file_obj.read()
                        extracted_text = content_bytes.decode('latin-1') # or 'cp1252'
                    except UnicodeDecodeError:
                        extracted_text = "[Error: Could not decode .txt file content. Tried UTF-8 and Latin-1.]"
                        document.status = Document.ProcessingStatus.FAILED
                        document.error_message = extracted_text
                except Exception as e:
                    extracted_text = f"[Error reading .txt file: {str(e)}]"
                    document.status = Document.ProcessingStatus.FAILED
                    document.error_message = extracted_text
            
            elif document.file_type == 'pdf':
                if PYPDF_AVAILABLE:
                    file_obj.seek(0)
                    try:
                        # PdfReader works with file-like objects. For in-memory files (like UploadedFile),
                        # wrapping it in BytesIO can be more robust if direct passing causes issues,
                        # though often file_obj itself works.
                        # Using document.original_file.path would be for files already saved and accessed from disk.
                        # Since file_obj is passed, it's the in-memory uploaded file.
                        pdf_reader = PdfReader(file_obj) 
                        text_parts = []
                        for page in pdf_reader.pages:
                            page_text = page.extract_text()
                            if page_text:
                                text_parts.append(page_text)
                        extracted_text = "\n".join(text_parts) if text_parts else "[No text found in PDF]"
                        if not text_parts:
                             document.error_message = "[No text could be extracted from PDF pages.]"
                             # Keep status as COMPLETED if no error, but text is empty.
                             # Or change to FAILED if empty text is an error for your use case.
                    except Exception as e:
                        extracted_text = f"[Error processing PDF file with pypdf: {str(e)}]"
                        document.status = Document.ProcessingStatus.FAILED
                        document.error_message = extracted_text
                else:
                    extracted_text = "[Error: PDF processing library (pypdf) not available on server.]"
                    document.status = Document.ProcessingStatus.FAILED
                    document.error_message = extracted_text
            
            elif document.file_type == 'docx':
                if PYTHON_DOCX_AVAILABLE:
                    file_obj.seek(0)
                    try:
                        doc = docx.Document(file_obj)
                        text_parts = [para.text for para in doc.paragraphs if para.text]
                        extracted_text = "\n".join(text_parts) if text_parts else "[No text found in DOCX]"
                        if not text_parts:
                            document.error_message = "[No text could be extracted from DOCX paragraphs.]"
                    except Exception as e:
                        extracted_text = f"[Error processing DOCX file with python-docx: {str(e)}]"
                        document.status = Document.ProcessingStatus.FAILED
                        document.error_message = extracted_text
                else:
                    extracted_text = "[Error: DOCX processing library (python-docx) not available on server.]"
                    document.status = Document.ProcessingStatus.FAILED
                    document.error_message = extracted_text
            
            # Add more elif for other file types (md, etc.) here
            # Example for a .md file (requires `markdown` and `bs4` pip installs)
            # elif document.file_type == 'md':
            #     if MARKDOWN_AVAILABLE and BS4_AVAILABLE:
            #         file_obj.seek(0)
            #         md_content = file_obj.read().decode('utf-8')
            #         html = markdown.markdown(md_content)
            #         soup = BeautifulSoup(html, 'html.parser')
            #         extracted_text = soup.get_text()
            #     else:
            #         extracted_text = "[Markdown processing libraries not available]"
            #         document.status = Document.ProcessingStatus.FAILED
            #         document.error_message = extracted_text

            document.extracted_text = extracted_text
            if document.status != Document.ProcessingStatus.FAILED: # Don't override FAILED status
                document.status = Document.ProcessingStatus.COMPLETED
            document.processed_at = timezone.now()
            document.save(update_fields=['extracted_text', 'status', 'processed_at', 'error_message'])
        except Exception as e: # Catch-all for unexpected errors during the try block
            if document.status != Document.ProcessingStatus.FAILED: # Avoid overwriting specific failure
                document.status = Document.ProcessingStatus.FAILED
            document.error_message = f"[Unexpected error in _process_document_content: {str(e)}]"
            document.processed_at = timezone.now()
            document.save(update_fields=['status', 'error_message', 'processed_at', 'extracted_text']) # also save extracted_text which might contain partial error info

    @action(detail=True, methods=['post'], url_path='search')
    def search_in_kb(self, request, pk=None):
        knowledge_base = self.get_object() # Handles permission via IsOwnerOrReadOnly for SAFE methods
        # If KB is private, IsOwnerOrReadOnly ensures only owner can access.
        # If KB is public, any authenticated user can access.

        query = request.data.get('query', '').strip()
        if not query:
            return Response({'detail': 'Query parameter is required and cannot be empty.'}, status=status.HTTP_400_BAD_REQUEST)

        documents = Document.objects.filter(
            Q(knowledge_base=knowledge_base) &
            Q(status=Document.ProcessingStatus.COMPLETED) &
            (Q(extracted_text__icontains=query) | Q(file_name__icontains=query))
        ).order_by('-uploaded_at').distinct()
        
        serializer = DocumentSerializer(documents, many=True, context={'request': request})
        return Response(serializer.data)

    @action(detail=True, methods=['delete'], url_path=r'documents/(?P<document_pk>[^/.]+)')
    def delete_document_from_kb(self, request, pk=None, document_pk=None):
        knowledge_base = self.get_object()
        # IsOwnerOrReadOnly ensures only the owner of the KB can perform this action (delete).
        document = get_object_or_404(Document, pk=document_pk, knowledge_base=knowledge_base)
        document.delete()
        return Response(status=status.HTTP_204_NO_CONTENT)

# Commented out DocumentViewSet section is now fully removed.