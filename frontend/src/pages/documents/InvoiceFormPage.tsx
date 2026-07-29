import React, { useState, useCallback, useRef, useEffect } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { Plus, Trash2, Search, ArrowLeft, Save, CheckCircle, Package, ImageUp, Loader2, Camera, Image as ImageIcon, Percent, DollarSign, Hash } from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import { useCreateDocument, useConfirmDocument } from '@/hooks/useDocuments';
import { useAllSuppliers } from '@/hooks/useSuppliers';
import { useSearchProducts, useCreateProduct } from '@/hooks/useProducts';
import { Category, VatRate, UnitOfMeasure } from '@/types/product';
import { useCategoryTree } from '@/hooks/useCategories';
import { useSuppliers } from '@/hooks/useSuppliers';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Select } from '@/components/ui/Select';
import { formatCurrency } from '@/utils/format';
import toast from 'react-hot-toast';

import { useBackNavigation } from '@/hooks/useBackNavigation';
import api from '@/services/api';

/** Спосіб оплати з постачальником */
type PaymentMethod = 'credit' | 'bank_transfer' | 'cash' | 'other';

const PAYMENT_METHODS: { value: PaymentMethod; label: string }[] = [
  { value: 'credit', label: 'В борг постачальнику' },
  { value: 'bank_transfer', label: 'По перерахунку' },
  { value: 'cash', label: 'Готівкою з каси' },
  { value: 'other', label: 'Інший спосіб' },
];

interface CartItem {
  product_id: string;
  product_title: string;
  product_barcode: string | null;
  quantity: number;
  /** Ціна продажу = собівартість × (1 + націнка/100), заокруглена до гривні */
  price: number;
  /** Собівартість = ціна з ПДВ з накладної */
  cost_price: number;
  /** Націнка (%) — підтягується з карточки товару */
  markup_percent: number;
}

/** Заокруглення ціни продажу до гривні (без копійок) */
const roundPrice = (value: number): number => Math.round(value);

/** Розрахувати markup % з retail_price та cost_price */
const calcMarkupPercent = (retailPrice: number, costPrice: number): number => {
  if (costPrice <= 0) return 0;
  return Math.round(((retailPrice - costPrice) / costPrice) * 100);
};

interface NewProductFormState {
  additional_barcode: string;
  title: string;
  barcode: string;
  sku: string;
  uktzed: string;
  price: string;
  cost_price: string;
  markup: string;
  stock: string;
  recommended_qty: string;
  category_id: string | null;
  supplier_id: string | null;
  tax_rate: VatRate;
  unit: string;
  is_weight: boolean;
  scan_excise: boolean;
}

/** Рекурсивно будує список опцій категорій */
function buildCategoryOptions(categories: Category[], depth: number = 0): { value: string; label: string; disabled?: boolean }[] {
  const options: { value: string; label: string; disabled?: boolean }[] = [];
  for (const cat of categories) {
    const hasChildren = cat.children && cat.children.length > 0;
    if (hasChildren) {
      options.push({ value: cat.id, label: `${'  '.repeat(depth)}▶ ${cat.name}`, disabled: true });
      options.push(...buildCategoryOptions(cat.children!, depth + 1));
    } else {
      options.push({ value: cat.id, label: `${'  '.repeat(depth)}└── ${cat.name}`, disabled: false });
    }
  }
  return options;
}

const InvoiceFormPage: React.FC = () => {
  const navigate = useNavigate();
  const { id: editId } = useParams<{ id: string }>();
  const isEdit = !!editId;
  const { goBack } = useBackNavigation();
  const { data: suppliersData } = useAllSuppliers();
  const createMutation = useCreateDocument();
  const confirmMutation = useConfirmDocument();
  const createProductMutation = useCreateProduct();

  const [number, setNumber] = useState('');
  const [invoiceDate, setInvoiceDate] = useState(new Date().toISOString().split('T')[0]);
  const [isFiscal, setIsFiscal] = useState(false);
  const [supplierId, setSupplierId] = useState<string | null>(null);
  const [paymentMethod, setPaymentMethod] = useState<PaymentMethod | ''>('');
  const [notes, setNotes] = useState('');
  const [cart, setCart] = useState<CartItem[]>([]);
  /** Лічильник для унікальних ключів при однакових product_id */
  const [cartKeyCounter, setCartKeyCounter] = useState(0);

  // Завантаження даних для редагування
  const { data: editData } = useQuery({
    queryKey: ['invoice', editId],
    queryFn: async () => {
      if (!editId) return null;
      const response = await api.get(`/invoices/${editId}`);
      return response.data;
    },
    enabled: isEdit,
  });

  // Заповнення форми при редагуванні
  useEffect(() => {
    if (!editData) return;
    setNumber(editData.number || '');
    setInvoiceDate(editData.invoice_date ? editData.invoice_date.split('T')[0] : '');
    setIsFiscal(editData.is_fiscal || false);
    setSupplierId(editData.supplier_id || null);
    setPaymentMethod(editData.payment_method || '');
    setNotes(editData.notes || '');

    if (editData.items && editData.items.length > 0) {
      const cartItems: CartItem[] = editData.items.map((item: any) => {
        // Актуальна собівартість з карточки товару
        const currentCostPrice = parseFloat(item.product?.cost_price) || 0;
        const savedCostPrice = Number(item.cost_price || item.price || 0);
        const costPrice = currentCostPrice > 0 ? currentCostPrice : savedCostPrice;

        // Актуальна ціна продажу з карточки товару
        const currentRetailPrice = parseFloat(item.product?.price) || 0;
        const savedPrice = Number(item.price || 0);

        // Актуальна націнка з карточки товару
        const savedMarkup = parseFloat(item.markup_percent) || 0;
        const currentMarkup = parseFloat(item.product?.markup) || savedMarkup;

        // Ціна продажу: пріоритет — retail_price з БД
        const retailPrice = currentRetailPrice > 0 ? currentRetailPrice : savedPrice;

        // Націнка: якщо є retail_price, перераховуємо для узгодженості
        const markupPercent = retailPrice > 0 && costPrice > 0
          ? calcMarkupPercent(retailPrice, costPrice)
          : currentMarkup;

        // Ціна продажу = retail_price (заокруглена), або розрахована
        const price = retailPrice > 0
          ? roundPrice(retailPrice)
          : costPrice > 0
            ? roundPrice(costPrice * (1 + markupPercent / 100))
            : roundPrice(savedPrice);

        return {
          product_id: item.product_id,
          product_title: item.product?.title || item.product_name || "",
          product_barcode: item.product?.barcode || null,
          quantity: Number(item.quantity),
          price,
          cost_price: costPrice,
          markup_percent: markupPercent,
        };
      });
      setCart(cartItems);
    }
  }, [editData]);

  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<any[]>([]);
  const [showSearch, setShowSearch] = useState(false);

  // Стан для аналізу фото накладної
  const [isAnalyzing, setIsAnalyzing] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Стан для модалки створення нового товару
  const [showNewProductModal, setShowNewProductModal] = useState(false);
  const [newProduct, setNewProduct] = useState<NewProductFormState>({
    title: '',
    barcode: '',
    sku: '',
    uktzed: '',
    price: '',
    cost_price: '',
    markup: '',
    stock: '0',
    recommended_qty: '',
    category_id: null,
    supplier_id: null,
    tax_rate: 20 as VatRate,
    unit: 'pcs',
    is_weight: false,
    scan_excise: false,
        additional_barcode: '',
  });

  // Стан для відстеження чи були зміни (для авто-збереження чернетки)
  const [isDirty, setIsDirty] = useState(false);
  const [draftId, setDraftId] = useState<string | null>(null);
  const autoSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isSavingRef = useRef(false);

  const { data: categoryTree } = useCategoryTree();
  const { data: suppliersDataForNew } = useSuppliers({ page: 1, size: 100 });
  const { data: searchData } = useSearchProducts(searchQuery);

  // ─── Автоматичне збереження чернетки ─────────────────────────────────────

  const saveDraft = useCallback(async () => {
    if (isSavingRef.current) return;
    // В режимі редагування — оновлюємо існуючу накладну через editId
    if (isEdit && editId) {
      // Оновлюємо тільки якщо заповнені всі обов'язкові поля
      if (!supplierId || cart.length === 0) return;

      try {
        const payload: any = {
          number: number.trim() || undefined,
          supplier_id: supplierId,
          invoice_date: new Date(invoiceDate).toISOString(),
          is_fiscal: isFiscal,
          payment_method: paymentMethod || undefined,
          notes: notes || undefined,
          items: cart.map(({ product_title, product_barcode, markup_percent, ...item }) => ({
            product_id: item.product_id,
            quantity: item.quantity,
            price: item.price,
            cost_price: item.cost_price,
            markup_percent,
            total: item.quantity * item.price,
          })),
        };

        await api.put(`/invoices/${editId}`, payload);
      } catch {
        // Мовчки ігноруємо помилки авто-збереження
      }
      return;
    }

    // Для нової накладної — створюємо чернетку (тільки якщо є постачальник)
    if (!supplierId) return;

    try {
      const payload: any = {
        document_type: 'invoice',
        number: number.trim() || undefined,
        supplier_id: supplierId,
        invoice_date: new Date(invoiceDate).toISOString(),
        is_fiscal: isFiscal,
        payment_method: paymentMethod || undefined,
        notes: notes || undefined,
      };

      if (cart.length > 0) {
        payload.items = cart.map(({ product_title, product_barcode, markup_percent, ...item }) => ({
          product_id: item.product_id,
          quantity: item.quantity,
          price: item.price,
          cost_price: item.cost_price,
          markup_percent,
          total: item.quantity * item.price,
        }));
      }

      if (draftId) {
        // Оновлюємо існуючу чернетку через PUT /invoices/{id}
        await api.put(`/invoices/${draftId}`, payload);
      } else {
        // Створюємо нову чернетку через POST /invoices
        const res = await api.post('/invoices', payload);
        if (res.data?.id) {
          setDraftId(res.data.id);
        }
      }
    } catch {
      // Мовчки ігноруємо помилки авто-збереження
    }
  }, [number, invoiceDate, isFiscal, supplierId, paymentMethod, notes, cart, draftId, isEdit, editId]);

  // Запускаємо авто-збереження через 3 секунди після останньої зміни
  useEffect(() => {
    if (!isDirty) return;

    if (autoSaveTimerRef.current) {
      clearTimeout(autoSaveTimerRef.current);
    }

    autoSaveTimerRef.current = setTimeout(() => {
      saveDraft();
      setIsDirty(false);
    }, 3000);

    return () => {
      if (autoSaveTimerRef.current) {
        clearTimeout(autoSaveTimerRef.current);
      }
    };
  }, [isDirty, saveDraft]);

  // Позначаємо форму як змінену
  const markDirty = useCallback(() => {
    setIsDirty(true);
  }, []);

  // ─── Пошук товару (пріоритет — штрих-код) ────────────────────────────────

  const handleSearch = useCallback(
    (query: string) => {
      setSearchQuery(query);
      if (query.length >= 2 && searchData?.items) {
        setSearchResults(searchData.items);
        setShowSearch(true);
      } else {
        setSearchResults([]);
        setShowSearch(false);
      }
    },
    [searchData]
  );

  // ─── Додавання товару в кошик ────────────────────────────────────────────

  const addToCart = (product: any) => {
    // Собівартість = ціна з ПДВ (з бази або 0)
    const costPrice = parseFloat(product.cost_price) || 0;

    // Ціна продажу = retail_price з карточки товару (заокруглена до гривні)
    const retailPrice = parseFloat(product.price) || 0;

    // Націнка (%) — розраховуємо з retail_price та cost_price
    const markupPercent = retailPrice > 0 && costPrice > 0
      ? calcMarkupPercent(retailPrice, costPrice)
      : parseFloat(product.markup) || 0;

    // Ціна продажу: пріоритет — retail_price з БД
    const price = retailPrice > 0
      ? roundPrice(retailPrice)
      : costPrice > 0
        ? roundPrice(costPrice * (1 + markupPercent / 100))
        : 0;

    const existing = cart.find((item) => item.product_id === product.id);
    if (existing) {
      setCart((prev) =>
        prev.map((item) =>
          item.product_id === product.id
            ? { ...item, quantity: item.quantity + 1 }
            : item
        )
      );
    } else {
      setCart((prev) => [
        ...prev,
        {
          product_id: product.id,
          product_title: product.title,
          product_barcode: product.barcode || null,
          quantity: 1,
          price,
          cost_price: costPrice,
          markup_percent: markupPercent,
        },
      ]);
      // Збільшуємо лічильник для унікальних ключів
      setCartKeyCounter((c) => c + 1);
    }
    setSearchQuery('');
    setShowSearch(false);
    markDirty();
  };

  // ─── Оновлення кількості ─────────────────────────────────────────────────

  const updateQuantity = (productId: string, quantity: number) => {
    if (quantity <= 0) {
      setCart((prev) => prev.filter((item) => item.product_id !== productId));
    } else {
      setCart((prev) =>
        prev.map((item) =>
          item.product_id === productId ? { ...item, quantity } : item
        )
      );
    }
    markDirty();
  };

  // ─── Оновлення собівартості → перерахунок ціни продажу ───────────────────

  const updateCostPrice = (productId: string, costPrice: number) => {
    setCart((prev) =>
      prev.map((item) => {
        if (item.product_id !== productId) return item;
        // Ціна продажу = собівартість × (1 + націнка/100), заокруглена до гривні
        const newPrice = costPrice > 0
          ? roundPrice(costPrice * (1 + item.markup_percent / 100))
          : 0;
        return { ...item, cost_price: costPrice, price: newPrice };
      })
    );
    markDirty();
  };

  // ─── Оновлення націнки → перерахунок ціни продажу ────────────────────────

  const updateMarkup = (productId: string, markupPercent: number) => {
    setCart((prev) =>
      prev.map((item) => {
        if (item.product_id !== productId) return item;
        // Ціна продажу = собівартість × (1 + націнка/100), заокруглена до гривні
        const newPrice = item.cost_price > 0
          ? roundPrice(item.cost_price * (1 + markupPercent / 100))
          : item.price;
        return { ...item, markup_percent: markupPercent, price: newPrice };
      })
    );
    markDirty();
  };

  // ─── Оновлення ціни продажу → перерахунок націнки ────────────────────────

  const updatePrice = (productId: string, price: number) => {
    setCart((prev) =>
      prev.map((item) => {
        if (item.product_id !== productId) return item;
        // Націнка = (ціна продажу - собівартість) / собівартість × 100
        const markup = item.cost_price > 0
          ? Math.round(((price - item.cost_price) / item.cost_price) * 100)
          : 0;
        return { ...item, price, markup_percent: markup };
      })
    );
    markDirty();
  };

  // ─── Видалення товару ────────────────────────────────────────────────────

  const removeFromCart = (productId: string) => {
    setCart((prev) => prev.filter((item) => item.product_id !== productId));
    markDirty();
  };

  // ─── Підрахунок підсумків ────────────────────────────────────────────────

  const totalAmount = cart.reduce((sum, item) => sum + item.quantity * item.price, 0);
  const totalCost = cart.reduce((sum, item) => sum + item.quantity * item.cost_price, 0);
  const totalMarkup = totalCost > 0 ? Math.round(((totalAmount - totalCost) / totalCost) * 100) : 0;

  // ─── Збереження накладної ────────────────────────────────────────────────
  const handleSave = async (andConfirm: boolean = false) => {
    isSavingRef.current = true;
    if (autoSaveTimerRef.current) clearTimeout(autoSaveTimerRef.current);
    

    if (!supplierId) {
      toast.error("Виберіть постачальника");
      isSavingRef.current = false;
      return;
    }
    if (cart.length === 0) {
      toast.error("Додайте хоча б один товар");
      isSavingRef.current = false;
      return;
    }
    
    try {
      if (isEdit) {
        const payload = {
          number: number.trim() || undefined,
          supplier_id: supplierId,
          invoice_date: new Date(invoiceDate).toISOString(),
          payment_method: paymentMethod || undefined,
          is_fiscal: isFiscal,
          notes: notes || undefined,
          items: cart.map(({ product_title, product_barcode, markup_percent, ...item }) => ({
            product_id: item.product_id,
            quantity: item.quantity,
            price: item.price,
            cost_price: item.cost_price,
            markup_percent,
            total: item.quantity * item.price,
          })),
        };
        
        await api.put(`/invoices/${editId}`, payload);
        toast.success("Накладну оновлено");
        navigate("/documents");
        return;
      }
      
      const doc = await createMutation.mutateAsync({
        document_type: "invoice",
        number: number.trim() || undefined,
        supplier_id: supplierId,
        invoice_date: new Date(invoiceDate).toISOString(),
        payment_method: paymentMethod || undefined,
        is_fiscal: isFiscal,
        notes: notes || undefined,
        items: cart.map(({ product_title, product_barcode, markup_percent, ...item }) => ({
          product_id: item.product_id,
          quantity: item.quantity,
          price: item.price,
          cost_price: item.cost_price,
          markup_percent,
          total: item.quantity * item.price,
        })),
      });
      
      if (andConfirm) {
        await confirmMutation.mutateAsync({ id: doc.id, documentType: "invoice" });
      }
      
      navigate("/documents");
    } catch (err: any) {
      const detail = err?.response?.data?.detail || err?.message || "Помилка при збереженні";
      toast.error(detail);
    } finally {
    }
  };
  const handleImageUpload = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    setIsAnalyzing(true);
    try {
      const formData = new FormData();
      formData.append('file', file);

      // Використовуємо ендпоінт /invoice-ocr/analyze,
      // який автоматично зіставляє товари з БД за штрих-кодом
      const response = await api.post('/invoice-ocr/analyze', formData);
      const result = response.data;

      if (result.success) {
        const data = result.data;

        // Заповнити поля форми
        if (data.document_number) setNumber(data.document_number);
        if (data.invoice_date) setInvoiceDate(data.invoice_date);
        if (data.is_fiscal !== null) setIsFiscal(data.is_fiscal);
        if (data.supplier_name) {
          // Знайти постачальника за назвою серед suppliersData
          const supplier = suppliersData?.find(
            s => s.name.toLowerCase().includes(data.supplier_name.toLowerCase())
          );
          if (supplier) setSupplierId(String(supplier.id));
        }
        if (data.payment_method) setPaymentMethod(data.payment_method);

        // Додати товари з автоматичним зіставленням
        if (data.items && data.items.length > 0) {
          const newCart: CartItem[] = data.items.map((item: any) => {
            // Собівартість = ціна з ПДВ з накладної
            const costPrice = parseFloat(item.cost_price) || 0;

            // Націнка (%) — підтягуємо з карточки товару напряму
            const markupPercent = parseFloat(item.markup_percent) || 0;

            // Ціна продажу = собівартість × (1 + націнка/100), заокруглена до гривні
            const price = costPrice > 0
              ? roundPrice(costPrice * (1 + markupPercent / 100))
              : 0;

            // Якщо товар знайдено в БД — використовуємо його ID
            const productId = item.matched_product_id || '';
            const productTitle = item.matched_product_name || item.product_name;
            const productBarcode = item.matched_barcode || null;

            return {
              product_id: productId,
              product_title: productTitle,
              product_barcode: productBarcode,
              quantity: item.quantity,
              price,
              cost_price: costPrice,
              markup_percent: markupPercent,
            };
          });
          setCart(newCart);
          setCartKeyCounter(newCart.length);

          // Показати інформацію про зіставлення
          const matchedCount = data.items.filter((i: any) => i.matched_product_id).length;
          const notFoundCount = data.items.length - matchedCount;
          if (notFoundCount > 0) {
            toast.success(
              `Знайдено ${data.items.length} товарів. ${matchedCount} зіставлено з БД, ${notFoundCount} не знайдено — додано без прив'язки.`
            );
          } else {
            toast.success(`Знайдено ${data.items.length} товарів. Всі зіставлено з БД!`);
          }
        }

        toast.success('Накладну розпізнано!');
        markDirty();
      } else {
        toast.error(result.error || 'Помилка аналізу накладної');
      }
    } catch (err: any) {
      // Безпечне отримання деталей помилки
      let detail = "Помилка з'єднання";
      try {
        const data = err?.response?.data;
        if (data) {
          if (typeof data.detail === 'string') {
            detail = data.detail;
          } else if (Array.isArray(data.detail)) {
            detail = data.detail.map((d: any) => d.msg || String(d)).join('; ');
          } else if (data.error) {
            detail = data.error;
          }
        }
      } catch {
        // ignore parse errors
      }
      toast.error(detail);
    } finally {
      setIsAnalyzing(false);
      // Скинути input, щоб можна було вибрати той самий файл повторно
      e.target.value = '';
    }
  };

  // ─── Створення нового товару ─────────────────────────────────────────────

  const handleCreateProduct = async () => {
    if (!newProduct.title.trim()) {
      toast.error('Введіть назву товару');
      return;
    }
    
    const costPrice = parseFloat(newProduct.cost_price) || 0;
    const markup = parseFloat(newProduct.markup) || 0;
    
    // Розрахувати ціну з собівартості та націнки, якщо ціна не вказана
    let price = parseFloat(newProduct.price) || 0;
    if (price <= 0 && costPrice > 0 && markup > 0) {
      price = Math.round(costPrice * (1 + markup / 100));
    }

    try {
      const product = await createProductMutation.mutateAsync({
        title: newProduct.title.trim(),
        barcode: newProduct.barcode.trim() || undefined,
        sku: newProduct.sku.trim() || undefined,
        uktzed: newProduct.uktzed.trim() || undefined,
        price: price || 0,
        cost_price: costPrice || 0,
        markup: markup || undefined,
        stock: parseInt(newProduct.stock) || 0,
        recommended_qty: parseInt(newProduct.recommended_qty) || undefined,
        category_id: newProduct.category_id || undefined,
        supplier_id: newProduct.supplier_id || undefined,
        tax_rate: newProduct.tax_rate as VatRate,
        unit: (newProduct.unit || 'pcs') as UnitOfMeasure,
        is_weight: newProduct.is_weight,
        scan_excise: newProduct.scan_excise,
      });
      // Додаємо новий товар одразу в кошик
      addToCart(product);

      // Якщо вказано додатковий штрих-код — додаємо через API
      if (newProduct.additional_barcode.trim() && product.id) {
        try {
          await api.post(`/products/${product.id}/barcodes`, {
            barcode: newProduct.additional_barcode.trim(),
          });
        } catch {
          // Мовчки ігноруємо помилку додавання коду
        }
      }

      setShowNewProductModal(false);
      setNewProduct({
        title: '', barcode: '', sku: '', uktzed: '', price: '', cost_price: '', markup: '',
        stock: '0', recommended_qty: '', category_id: null, supplier_id: null,
        tax_rate: 20 as VatRate, unit: 'pcs', is_weight: false, scan_excise: false,
        additional_barcode: '',
      });
      toast.success('Товар створено та додано до накладної');
    } catch {
      // Error handled
    }
  };

  // ─── Опції для Select ────────────────────────────────────────────────────

  const supplierOptions = [
    { value: '', label: 'Виберіть постачальника' },
    ...(suppliersData?.map((s) => ({
      value: String(s.id),
      label: s.name,
    })) || []),
  ];

  const paymentMethodOptions = [
    { value: '', label: 'Не вибрано' },
    ...PAYMENT_METHODS.map((pm) => ({
      value: pm.value,
      label: pm.label,
    })),
  ];

  const taxRateOptions = [
    { value: 0, label: '0%' },
    { value: 5, label: '5%' },
    { value: 7, label: '7%' },
    { value: 20, label: '20%' },
  ];

  const unitOptions = [
    { value: 'pcs', label: 'шт' },
    { value: 'kg', label: 'кг' },
    { value: 'l', label: 'л' },
    { value: 'm', label: 'м' },
    { value: 'box', label: 'кор' },
    { value: 'pack', label: 'уп' },
  ];

  // ─── Рендер ──────────────────────────────────────────────────────────────

  return (
    <div className="max-w-5xl mx-auto space-y-6">
      {/* Заголовок */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-4">
          <button
            onClick={goBack}
            className="p-2 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
          >
            <ArrowLeft className="w-5 h-5" />
          </button>
          <div>
            <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
              {isEdit ? 'Редагування' : 'Нова'} прибуткова накладна
            </h2>
            <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
              {isEdit ? 'Редагування прибуткової накладної' : 'Створення прибуткової накладної'}
            </p>
          </div>
        </div>
      </div>

      {/* Прихований input для вибору файлу */}
      <input
        type="file"
        accept="image/*"
        className="hidden"
        ref={fileInputRef}
        onChange={handleImageUpload}
      />

      <div className="card p-6 space-y-6">
        {/* ─── Рядок 1: Номер, Дата, Фіскальна + кнопка фото ───────────── */}
        <div className="grid grid-cols-1 md:grid-cols-4 gap-4">
          <Input
            label="Номер накладної *"
            value={number}
            onChange={(e) => { setNumber(e.target.value); markDirty(); }}
            placeholder="Наприклад: ПН-001"
            autoFocus
          />
          <Input
            label="Дата накладної"
            type="date"
            value={invoiceDate}
            onChange={(e) => { setInvoiceDate(e.target.value); markDirty(); }}
          />

          {/* Блок "Фіскальна накладна" */}
          <div className="flex flex-col justify-end">
            <label className="flex items-center gap-2 cursor-pointer py-2">
              <input
                type="checkbox"
                checked={isFiscal}
                onChange={(e) => { setIsFiscal(e.target.checked); markDirty(); }}
                className="w-4 h-4 rounded border-gray-300 dark:border-slate-600 text-blue-600 focus:ring-blue-500"
              />
              <span className="text-sm font-medium text-gray-700 dark:text-gray-300">
                Фіскальна накладна
              </span>
            </label>
          </div>

          {/* Кнопка "Завантажити фото накладної" — після блоку "Фіскальна накладна" */}
          <div className="flex items-end">
            <button
              onClick={() => fileInputRef.current?.click()}
              disabled={isAnalyzing}
              className="w-full flex items-center justify-center gap-2 px-4 py-2.5 rounded-xl
                         bg-primary-50 dark:bg-primary-900/20
                         text-primary-600 dark:text-primary-400
                         hover:bg-primary-100 dark:hover:bg-primary-900/30
                         disabled:opacity-50 disabled:cursor-not-allowed
                         transition-all duration-200 border border-primary-200 dark:border-primary-800
                         shadow-sm hover:shadow-md text-sm font-medium"
            >
              {isAnalyzing ? (
                <>
                  <Loader2 className="w-4 h-4 animate-spin" />
                  <span>Аналіз фото...</span>
                </>
              ) : (
                <>
                  <Camera className="w-4 h-4" />
                  <span>Завантажити фото накладної</span>
                </>
              )}
            </button>
          </div>
        </div>

        {/* ─── Рядок 2: Постачальник, Спосіб оплати ────────────────────── */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <Select
            label="Постачальник *"
            options={supplierOptions}
            value={String(supplierId || '')}
            onChange={(e) => { setSupplierId(e.target.value || null); markDirty(); }}
          />
          <Select
            label="Спосіб оплати"
            options={paymentMethodOptions}
            value={paymentMethod}
            onChange={(e) => { setPaymentMethod(e.target.value as PaymentMethod | ''); markDirty(); }}
          />
        </div>

        {/* ─── Пошук товару + кнопка додати новий ──────────────────────── */}
        <div className="flex gap-3 items-end">
          <div className="flex-1 relative">
            <Input
              label="Додати товар"
              value={searchQuery}
              onChange={(e) => handleSearch(e.target.value)}
              placeholder="Пошук за штрих-кодом або назвою..."
              icon={<Search className="w-4 h-4" />}
            />
            {showSearch && searchResults.length > 0 && (
              <div className="absolute z-10 w-full mt-1 bg-white dark:bg-slate-800 border border-gray-200 dark:border-slate-700 rounded-xl shadow-lg max-h-60 overflow-y-auto">
                {searchResults.map((product) => (
                  <button
                    key={product.id}
                    onClick={() => addToCart(product)}
                    className="w-full flex items-center justify-between px-4 py-2.5 hover:bg-gray-50 dark:hover:bg-slate-700 text-left transition-colors"
                  >
                    <div>
                      <p className="text-sm font-medium text-gray-900 dark:text-gray-100">
                        {product.title}
                      </p>
                      {product.barcode && (
                        <p className="text-xs text-gray-400">ШК: {product.barcode}</p>
                      )}
                    </div>
                    <div className="text-right">
                      <p className="text-sm font-medium text-gray-900 dark:text-gray-100">
                        {formatCurrency(product.price)}
                      </p>
                      <p className="text-xs text-gray-400">Залишок: {product.stock}</p>
                    </div>
                  </button>
                ))}
              </div>
            )}
          </div>
          <Button
            variant="secondary"
            onClick={() => setShowNewProductModal(true)}
            icon={<Package className="w-4 h-4" />}
          >
            Додати новий товар
          </Button>
        </div>

        {/* ─── Таблиця товарів ─────────────────────────────────────────── */}
        {cart.length > 0 && (
          <div className="border border-gray-200 dark:border-slate-700 rounded-xl overflow-hidden">
            <table className="w-full">
              <thead>
                <tr className="bg-gray-50 dark:bg-slate-800/50">
                  <th className="table-header">Товар</th>
                  <th className="table-header w-24">Кількість</th>
                  <th className="table-header w-28">Собівартість (з ПДВ)</th>
                  <th className="table-header w-28">Ціна продажу</th>
                  <th className="table-header w-28">Націнка</th>
                  <th className="table-header w-28">Сума собівартості</th>
                  <th className="table-header w-16"></th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
                {cart.map((item, index) => (
                  <tr key={`${item.product_id}-${index}-${cartKeyCounter}`}>
                    <td className="table-cell">
                      <p className="font-medium text-gray-900 dark:text-gray-100">
                        {item.product_title}
                      </p>
                      {item.product_barcode && (
                        <p className="text-xs text-gray-400">ШК: {item.product_barcode}</p>
                      )}
                      {!item.product_id && (
                        <p className="text-xs text-amber-500 font-medium">
                          ⚠️ Не знайдено в БД
                        </p>
                      )}
                    </td>
                    <td className="table-cell">
                      <input
                        type="number"
                        min="1"
                        value={item.quantity}
                        onChange={(e) =>
                          updateQuantity(item.product_id, parseInt(e.target.value) || 1)
                        }
                        className="w-20 input-field text-center px-3 no-spinner"
                      />
                    </td>
                    <td className="table-cell">
                      <input
                        type="number"
                        step="0.01"
                        min="0"
                        value={item.cost_price}
                        onChange={(e) =>
                          updateCostPrice(item.product_id, parseFloat(e.target.value) || 0)
                        }
                        className="w-24 input-field text-right px-3 no-spinner"
                        title="Собівартість = ціна з ПДВ з накладної"
                      />
                    </td>
                    <td className="table-cell">
                      <input
                        type="number"
                        step="1"
                        min="0"
                        value={item.price}
                        onChange={(e) =>
                          updatePrice(item.product_id, parseFloat(e.target.value) || 0)
                        }
                        className="w-24 input-field text-right px-3 no-spinner"
                        title="Ціна продажу заокруглена до гривні"
                      />
                    </td>
                    <td className="table-cell">
                      <div className="flex items-center gap-1">
                        <input
                          type="number"
                          step="0.1"
                          min="0"
                          value={item.markup_percent}
                          onChange={(e) =>
                            updateMarkup(item.product_id, parseFloat(e.target.value) || 0)
                          }
                          className="w-36 input-field text-right px-3 no-spinner"
                        />
                        <span className="text-sm text-gray-400">%</span>
                      </div>
                    </td>
                    <td className="table-cell font-medium">
                      {formatCurrency(item.quantity * item.cost_price)}
                    </td>
                    <td className="table-cell">
                      <button
                        onClick={() => removeFromCart(item.product_id)}
                        className="p-1.5 rounded-lg text-gray-400 hover:text-danger-600 hover:bg-danger-50 transition-colors"
                      >
                        <Trash2 className="w-4 h-4" />
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
              <tfoot>
                <tr className="bg-gray-50 dark:bg-slate-800/50">
                  <td colSpan={5} className="px-4 py-3 text-right text-gray-500 dark:text-gray-400 text-sm">
                    Закупівельна сума:
                  </td>
                  <td colSpan={2} className="px-4 py-3 font-bold text-lg text-gray-900 dark:text-gray-100">
                    {formatCurrency(totalCost)}
                  </td>
                </tr>
                <tr className="bg-gray-100 dark:bg-slate-800 border-t border-gray-300 dark:border-slate-600">
                  <td colSpan={5} className="px-4 py-2 text-right text-gray-500 dark:text-gray-400 text-sm">
                    Загальна сума (з націнкою):
                  </td>
                  <td colSpan={2} className="px-4 py-2 text-sm text-gray-600 dark:text-gray-400">
                    {formatCurrency(totalAmount)}
                  </td>
                </tr>
              </tfoot>
            </table>
          </div>
        )}

        {/* ─── Примітки ────────────────────────────────────────────────── */}
        <Input
          label="Примітки"
          value={notes}
          onChange={(e) => { setNotes(e.target.value); markDirty(); }}
          placeholder="Додаткова інформація..."
        />

        {/* ─── Кнопки дій ──────────────────────────────────────────────── */}
        <div className="flex justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
          <Button variant="secondary" onClick={goBack}>
            Скасувати
          </Button>
          <Button
            variant="secondary"
            onClick={() => handleSave(false)}
            icon={<Save className="w-4 h-4" />}
            isLoading={createMutation.isPending}
          >
            Зберегти як чернетку
          </Button>
          <Button
            onClick={() => handleSave(true)}
            icon={<CheckCircle className="w-4 h-4" />}
            isLoading={createMutation.isPending || confirmMutation.isPending}
          >
            Створити та підтвердити
          </Button>
        </div>
      </div>

      {/* ─── Модалка створення нового товару ───────────────────────────── */}
      {showNewProductModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={() => setShowNewProductModal(false)}>
          <div className="bg-white dark:bg-slate-800 rounded-2xl shadow-xl w-full max-w-4xl mx-4 p-6 space-y-5 overflow-y-auto max-h-[90vh]" onClick={(e) => e.stopPropagation()}>
            <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
              Новий товар
            </h3>
            <p className="text-sm text-gray-500 dark:text-gray-400 -mt-3">
              Заповніть інформацію про товар
            </p>

            {/* ═══════════════════════════════════════════════════════════════
               Фото товару та Основна інформація — в один рядок
               ═══════════════════════════════════════════════════════════════ */}
            <div className="flex gap-6">
              {/* Фото товару — ліворуч */}
              <div className="flex-shrink-0">
                <div className="w-32 h-32 rounded-xl border-2 border-dashed border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800/30 flex flex-col items-center justify-center">
                  <ImageIcon className="w-8 h-8 text-gray-300 dark:text-gray-600" />
                  <span className="mt-1 text-xs text-gray-300 dark:text-gray-600">Фото</span>
                </div>
              </div>

              {/* Поля інформації — праворуч */}
              <div className="flex-1 space-y-4">
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  <Input
                    label="Назва товару *"
                    value={newProduct.title}
                    onChange={(e) => setNewProduct((p) => ({ ...p, title: e.target.value }))}
                    placeholder="Введіть назву"
                    autoFocus
                  />
                  <Input
                    label="Штрих-код"
                    value={newProduct.barcode}
                    onChange={(e) => setNewProduct((p) => ({ ...p, barcode: e.target.value }))}
                    placeholder="13 цифр"
                  />
                </div>

                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  <Input
                    label="Артикул"
                    value={newProduct.sku}
                    onChange={(e) => setNewProduct((p) => ({ ...p, sku: e.target.value }))}
                    placeholder="Артикул товару"
                  />
                  <Select
                    label="Категорія"
                    options={[
                      { value: '', label: 'Без категорії' },
                      ...(categoryTree ? (() => {
                        const buildOpts = (cats: Category[], depth = 0): { value: string; label: string; disabled?: boolean }[] => {
                          const opts: { value: string; label: string; disabled?: boolean }[] = [];
                          for (const cat of cats) {
                            const hasChildren = cat.children && cat.children.length > 0;
                            if (hasChildren) {
                              opts.push({ value: cat.id, label: `${'  '.repeat(depth)}▶ ${cat.name}`, disabled: true });
                              opts.push(...buildOpts(cat.children!, depth + 1));
                            } else {
                              opts.push({ value: cat.id, label: `${'  '.repeat(depth)}└── ${cat.name}`, disabled: false });
                            }
                          }
                          return opts;
                        };
                        return buildOpts(categoryTree);
                      })() : [])
                    ]}
                    value={String(newProduct.category_id || '')}
                    onChange={(e) => setNewProduct((p) => ({ ...p, category_id: e.target.value || null }))}
                  />
                </div>
              </div>
            </div>

            {/* ═══════════════════════════════════════════════════════════════
               ЦІНИ ТА ФІНАНСИ — три взаємопов'язані поля
               ═══════════════════════════════════════════════════════════════ */}
            <div className="border-t border-gray-200 dark:border-slate-700 pt-4">
              <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
                Ціни та фінанси
              </h3>

              <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
                {/* Собівартість */}
                <div>
                  <Input
                    label="Собівартість"
                    type="number"
                    step="0.01"
                    min="0"
                    value={newProduct.cost_price}
                    onChange={(e) => {
                      const cost = e.target.value;
                      setNewProduct((p) => {
                        const markup = parseFloat(p.markup) || 0;
                        const costNum = parseFloat(cost) || 0;
                        const calculatedPrice = costNum > 0 && markup > 0 ? Math.round(costNum * (1 + markup / 100)) : Number(p.price);
                        return { ...p, cost_price: cost, price: calculatedPrice > 0 ? String(calculatedPrice) : p.price };
                      });
                    }}
                    icon={<DollarSign className="w-4 h-4 text-gray-400" />}
                  />
                  <button
                    type="button"
                    onClick={() => {
                      setNewProduct((p) => {
                        const cost = parseFloat(p.cost_price) || 0;
                        const newCost = Math.round(cost * 1.2 * 100) / 100;
                        const costStr = String(newCost);
                        const markup = parseFloat(p.markup) || 0;
                        const calculatedPrice = newCost > 0 && markup > 0 ? Math.round(newCost * (1 + markup / 100)) : Number(p.price);
                        return { ...p, cost_price: costStr, price: calculatedPrice > 0 ? String(calculatedPrice) : p.price };
                      });
                    }}
                    className="mt-1.5 inline-flex items-center gap-1 px-2.5 py-1 text-xs font-medium rounded-md
                      bg-green-50 text-green-700 hover:bg-green-100 
                      dark:bg-green-900/20 dark:text-green-400 dark:hover:bg-green-900/30
                      transition-colors"
                  >
                    <Plus className="w-3 h-3" />
                    +20% (ПДВ)
                  </button>
                </div>

                {/* Націнка (%) */}
                <Input
                  label="Націнка (%)"
                  type="number"
                  step="0.01"
                  min="0"
                  value={newProduct.markup}
                  onChange={(e) => {
                    const mark = e.target.value;
                    setNewProduct((p) => {
                      const costPrice = parseFloat(p.cost_price) || 0;
                      const markNum = parseFloat(mark) || 0;
                      const calculatedPrice = costPrice > 0 && markNum > 0 ? Math.round(costPrice * (1 + markNum / 100)) : Number(p.price);
                      return { ...p, markup: mark, price: calculatedPrice > 0 ? String(calculatedPrice) : p.price };
                    });
                  }}
                  icon={<Percent className="w-4 h-4 text-gray-400" />}
                />

                {/* Ціна продажу */}
                <Input
                  label="Ціна продажу"
                  type="number"
                  step="1"
                  min="0"
                  value={newProduct.price}
                  onChange={(e) => {
                    const price = e.target.value;
                    setNewProduct((p) => {
                      const costPrice = parseFloat(p.cost_price) || 0;
                      const priceNum = parseFloat(price) || 0;
                      const calculatedMarkup = costPrice > 0 && priceNum > 0 ? Math.round(((priceNum - costPrice) / costPrice) * 100) : Number(p.markup);
                      return { ...p, price, markup: calculatedMarkup > 0 ? String(calculatedMarkup) : p.markup };
                    });
                  }}
                />
              </div>
            </div>

            {/* ═══════════════════════════════════════════════════════════════
               ОБЛІК
               ═══════════════════════════════════════════════════════════════ */}
            <div className="border-t border-gray-200 dark:border-slate-700 pt-4">
              <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
                Облік
              </h3>
              <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
                <Input
                  label="Кількість"
                  type="number"
                  min="0"
                  value={newProduct.stock}
                  onChange={(e) => setNewProduct((p) => ({ ...p, stock: e.target.value }))}
                />
                <Input
                  label="Рекомендований залишок"
                  type="number"
                  min="0"
                  value={newProduct.recommended_qty}
                  onChange={(e) => setNewProduct((p) => ({ ...p, recommended_qty: e.target.value }))}
                  helperText="Мінімальний залишок для замовлення"
                />
                <Select
                  label="Постачальник"
                  options={[
                    { value: '', label: 'Без постачальника' },
                    ...(suppliersDataForNew?.items?.map((sup: any) => ({
                      value: String(sup.id),
                      label: sup.name,
                    })) || []),
                  ]}
                  value={String(newProduct.supplier_id || '')}
                  onChange={(e) => setNewProduct((p) => ({ ...p, supplier_id: e.target.value || null }))}
                />
              </div>
            </div>

            {/* ═══════════════════════════════════════════════════════════════
               ПОДАТКИ ТА ОДИНИЦІ ВИМІРУ
               ═══════════════════════════════════════════════════════════════ */}
            <div className="border-t border-gray-200 dark:border-slate-700 pt-4">
              <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
                Податки та одиниці виміру
              </h3>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <Select
                  label="Ставка податків"
                  options={[
                    { value: 0, label: '0%' },
                    { value: 5, label: '5%' },
                    { value: 7, label: '7%' },
                    { value: 20, label: '20%' },
                  ]}
                  value={newProduct.tax_rate}
                  onChange={(e) => setNewProduct((p) => ({ ...p, tax_rate: Number(e.target.value) as VatRate }))}
                />
                <Select
                  label="Одиниця виміру"
                  options={[
                    { value: 'pcs', label: 'шт' },
                    { value: 'kg', label: 'кг' },
                    { value: 'l', label: 'л' },
                    { value: 'm', label: 'м' },
                    { value: 'box', label: 'кор' },
                    { value: 'pack', label: 'уп' },
                  ]}
                  value={newProduct.unit}
                  onChange={(e) => setNewProduct((p) => ({ ...p, unit: e.target.value as UnitOfMeasure }))}
                />
              </div>
              <div className="mt-4">
                <Input
                  label="Код УКТЗЕД"
                  value={newProduct.uktzed}
                  onChange={(e) => setNewProduct((p) => ({ ...p, uktzed: e.target.value }))}
                  placeholder="10 цифр"
                  icon={<Hash className="w-4 h-4 text-gray-400" />}
                  helperText="Український класифікатор товарів зовнішньоекономічної діяльності"
                />
              </div>
            </div>

            {/* ═══════════════════════════════════════════════════════════════
               ДОДАТКОВІ ОПЦІЇ
               ═══════════════════════════════════════════════════════════════ */}
            <div className="border-t border-gray-200 dark:border-slate-700 pt-4">
              <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
                Додаткові опції
              </h3>
              <div className="space-y-3">
                <label className="flex items-center gap-3 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={newProduct.is_weight}
                    onChange={(e) => {
                      const checked = e.target.checked;
                      setNewProduct((p) => ({
                        ...p,
                        is_weight: checked,
                        unit: checked ? 'kg' : 'pcs',
                      }));
                    }}
                    className="w-4 h-4 rounded border-gray-300 text-primary-600 focus:ring-primary-500"
                  />
                  <span className="text-sm text-gray-700 dark:text-gray-300">
                    Ваговий товар (продаж за вагою)
                  </span>
                  {newProduct.is_weight ? (
                    <span className="text-xs text-amber-500">→ одиницю виміру змінено на кг</span>
                  ) : (
                    <span className="text-xs text-gray-400">→ одиниця виміру: шт</span>
                  )}
                </label>
                <label className="flex items-center gap-3 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={newProduct.scan_excise}
                    onChange={(e) => setNewProduct((p) => ({ ...p, scan_excise: e.target.checked }))}
                    className="w-4 h-4 rounded border-gray-300 text-primary-600 focus:ring-primary-500"
                  />
                  <span className="text-sm text-gray-700 dark:text-gray-300">
                    Сканувати акцизну марку
                  </span>
                </label>
              </div>
            </div>

            {/* ═══════════════════════════════════════════════════════════════
               ДОДАТКОВІ КОДИ
               ═══════════════════════════════════════════════════════════════ */}
            <div className="border-t border-gray-200 dark:border-slate-700 pt-4">
              <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
                Додаткові коди
              </h3>
              <div className="flex gap-2">
                <Input
                  label="Додатковий штрих-код"
                  value={newProduct.additional_barcode}
                  onChange={(e) => setNewProduct((p) => ({ ...p, additional_barcode: e.target.value }))}
                  placeholder="Введіть додатковий штрих-код"
                />
              </div>
              <p className="text-xs text-gray-400 mt-1">
                Додатковий штрих-код буде додано після створення товару
              </p>
            </div>

            {/* Кнопки */}
            <div className="flex justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
              <Button variant="secondary" onClick={() => setShowNewProductModal(false)}>
                Скасувати
              </Button>
              <Button onClick={handleCreateProduct} isLoading={createProductMutation.isPending}>
                Створити товар
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

export default InvoiceFormPage;
