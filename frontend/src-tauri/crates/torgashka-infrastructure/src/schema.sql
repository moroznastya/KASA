CREATE EXTENSION IF NOT EXISTS pg_trgm WITH SCHEMA public;

CREATE EXTENSION IF NOT EXISTS "uuid-ossp" WITH SCHEMA public;

CREATE TYPE public.enum_users_role AS ENUM (
    'admin',
    'cashier'
);

CREATE TYPE public.fiscal_status AS ENUM (
    'none',
    'pending',
    'sent',
    'failed',
    'fiscalized'
);

CREATE TYPE public.inventory_status AS ENUM (
    'draft',
    'confirmed',
    'cancelled'
);

CREATE TYPE public.invoice_status AS ENUM (
    'draft',
    'confirmed',
    'cancelled'
);

CREATE TYPE public.ledger_operation_type AS ENUM (
    'invoice',
    'payment',
    'return',
    'correction'
);

CREATE TYPE public.payment_method AS ENUM (
    'credit',
    'bank_transfer',
    'cash',
    'other'
);

CREATE TYPE public.prro_queue_status AS ENUM (
    'pending',
    'sent',
    'failed'
);

CREATE TYPE public.prro_shift_status AS ENUM (
    'open',
    'closed'
);

CREATE TYPE public.purchase_order_status AS ENUM (
    'draft',
    'confirmed',
    'cancelled'
);

CREATE TYPE public.receipt_payment_method AS ENUM (
    'cash',
    'card',
    'mixed'
);

CREATE TYPE public.receipt_type AS ENUM (
    'sale',
    'return'
);

CREATE TYPE public.return_action_type AS ENUM (
    'deduct_from_debt',
    'add_to_cash',
    'exchange'
);

CREATE TYPE public.return_invoice_status AS ENUM (
    'draft',
    'confirmed',
    'cancelled'
);

CREATE TYPE public.transfer_status AS ENUM (
    'draft',
    'confirmed',
    'cancelled'
);

CREATE TYPE public.user_role AS ENUM (
    'admin',
    'cashier',
    'owner',
    'store_manager'
);

CREATE TABLE IF NOT EXISTS public.barcodes (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    product_id uuid NOT NULL,
    barcode character varying(50) NOT NULL,
    is_primary boolean DEFAULT false NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.categories (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    name character varying(255) NOT NULL,
    description text,
    parent_id uuid,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.debtor_payments (
    id uuid NOT NULL,
    debtor_id uuid NOT NULL,
    amount numeric(12,2) NOT NULL,
    payment_method character varying(20),
    created_at timestamp without time zone NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.debtors (
    id uuid NOT NULL,
    name character varying(255) NOT NULL,
    phone character varying(50),
    notes text,
    total_debt numeric(12,2) NOT NULL,
    created_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.inventories (
    id uuid NOT NULL,
    number character varying(50) NOT NULL,
    location character varying(255),
    inventory_date timestamp without time zone NOT NULL,
    status public.inventory_status NOT NULL,
    notes text,
    created_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    created_by_id uuid NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.inventory_items (
    id uuid NOT NULL,
    inventory_id uuid NOT NULL,
    product_id uuid NOT NULL,
    actual_quantity numeric(10,3) NOT NULL,
    accounting_quantity numeric(10,3) NOT NULL,
    difference numeric(10,3) NOT NULL,
    cost_price numeric(12,2) NOT NULL,
    price numeric(12,2) NOT NULL,
    created_at timestamp without time zone NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.invoice_items (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    invoice_id uuid NOT NULL,
    product_id uuid NOT NULL,
    quantity numeric(10,3) NOT NULL,
    price numeric(10,2) NOT NULL,
    total numeric(12,2) NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    cost_price numeric(10,2),
    markup_percent numeric(5,1),
    previous_price numeric(10,2),
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.invoices (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    number character varying(50) NOT NULL,
    supplier_id uuid NOT NULL,
    invoice_date timestamp without time zone NOT NULL,
    status public.invoice_status DEFAULT 'draft'::public.invoice_status NOT NULL,
    notes text,
    total_amount numeric(12,2) DEFAULT 0.00,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL,
    is_fiscal boolean NOT NULL,
    payment_method public.payment_method,
    created_by_id uuid NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.print_templates (
    id uuid NOT NULL,
    name character varying(255) NOT NULL,
    type character varying(20) NOT NULL,
    content text NOT NULL,
    variables jsonb,
    is_default boolean NOT NULL,
    is_active boolean NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.product_images (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    product_id uuid NOT NULL,
    url character varying(1024) NOT NULL,
    is_main boolean DEFAULT false NOT NULL,
    sort_order integer DEFAULT 0 NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.products (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    barcode character varying(50),
    sku character varying(100),
    title character varying(255) NOT NULL,
    description text,
    price numeric(10,2) DEFAULT 0.00,
    cost_price numeric(10,2) DEFAULT 0.00,
    stock numeric(10,3) DEFAULT 0.000,
    uktzed character varying(10),
    scan_excise boolean DEFAULT false NOT NULL,
    tax_rate numeric(5,2) DEFAULT 20.00,
    tax_group character varying(2) DEFAULT 'А'::character varying,
    is_weight boolean DEFAULT false NOT NULL,
    unit character varying(10) DEFAULT 'шт'::character varying,
    category_id uuid,
    supplier_id uuid,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL,
    markup numeric(5,2),
    recommended_qty numeric(10,3),
    is_fiscal boolean DEFAULT false NOT NULL,
    fiscal_stock numeric(10,3) DEFAULT 0 NOT NULL
);

CREATE TABLE IF NOT EXISTS public.prro_queue_items (
    id uuid NOT NULL,
    store_id uuid NOT NULL,
    receipt_id uuid,
    shift_id uuid,
    local_number integer NOT NULL,
    check_type character varying(10) NOT NULL,
    xml_body text NOT NULL,
    mac text,
    status public.prro_queue_status NOT NULL,
    error text,
    created_at timestamp without time zone NOT NULL,
    sent_at timestamp without time zone
);

CREATE TABLE IF NOT EXISTS public.prro_settings (
    id integer NOT NULL,
    store_id uuid NOT NULL,
    key_name character varying(100) NOT NULL,
    value text,
    updated_at timestamp without time zone NOT NULL
);

CREATE SEQUENCE public.prro_settings_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.prro_settings_id_seq OWNED BY public.prro_settings.id;

CREATE TABLE IF NOT EXISTS public.prro_shifts (
    id uuid NOT NULL,
    store_id uuid NOT NULL,
    shift_number integer NOT NULL,
    opened_at timestamp without time zone NOT NULL,
    closed_at timestamp without time zone,
    signer_serial character varying(255),
    signer_name character varying(255),
    closed_by character varying(255),
    zreport_number character varying(50),
    status public.prro_shift_status NOT NULL,
    receipt_count integer NOT NULL,
    total_amount numeric(12,2) NOT NULL,
    last_local_number integer NOT NULL,
    last_mac text
);

CREATE TABLE IF NOT EXISTS public.purchase_order_items (
    id uuid NOT NULL,
    purchase_order_id uuid NOT NULL,
    product_id uuid NOT NULL,
    quantity numeric(10,3) NOT NULL,
    price numeric(10,2) NOT NULL,
    total numeric(12,2) NOT NULL,
    created_at timestamp without time zone NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.purchase_orders (
    id uuid NOT NULL,
    number character varying(50) NOT NULL,
    supplier_id uuid NOT NULL,
    order_date timestamp without time zone NOT NULL,
    expected_date timestamp without time zone,
    status public.purchase_order_status NOT NULL,
    is_fiscal boolean NOT NULL,
    notes text,
    total_amount numeric(12,2),
    invoice_id uuid,
    created_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    created_by_id uuid NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.receipt_items (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    receipt_id uuid NOT NULL,
    product_id uuid NOT NULL,
    quantity numeric(10,3) NOT NULL,
    price numeric(10,2) NOT NULL,
    total numeric(12,2) NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    purchase_price numeric(10,2),
    fiscal_quantity numeric(10,3) DEFAULT 0 NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.receipts (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    receipt_number character varying(50) NOT NULL,
    receipt_type public.receipt_type DEFAULT 'sale'::public.receipt_type NOT NULL,
    cashier_id uuid NOT NULL,
    total_amount numeric(12,2) NOT NULL,
    is_return boolean DEFAULT false NOT NULL,
    notes text,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    paid_amount numeric(12,2),
    debtor_id uuid,
    change_amount numeric(12,2),
    payment_method public.receipt_payment_method,
    original_receipt_id uuid,
    return_reason character varying(255),
    is_fiscal boolean DEFAULT false NOT NULL,
    fiscal_status public.fiscal_status DEFAULT 'none'::public.fiscal_status NOT NULL,
    fiscal_number character varying(50),
    fiscal_serial character varying(50),
    fiscal_sent_at timestamp without time zone,
    fiscal_error text,
    split_group_id uuid,
    cash_amount numeric(12,2),
    card_amount numeric(12,2),
    terminal_rrn character varying(32),
    terminal_approval_code character varying(16),
    terminal_invoice_number character varying(32),
    terminal_transaction_id character varying(64),
    terminal_response_code character varying(8),
    terminal_status character varying(16),
    terminal_receipt text,
    terminal_card_pan character varying(32),
    terminal_payment_system character varying(16),
    terminal_merchant character varying(32),
    terminal_created_at timestamp without time zone,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.return_invoice_items (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    return_invoice_id uuid NOT NULL,
    product_id uuid NOT NULL,
    quantity numeric(10,3) NOT NULL,
    price numeric(10,2) NOT NULL,
    total numeric(12,2) NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    cost_price numeric(10,2),
    markup_percent numeric(5,2),
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.return_invoices (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    number character varying(50) NOT NULL,
    supplier_id uuid NOT NULL,
    return_date timestamp without time zone NOT NULL,
    status public.return_invoice_status DEFAULT 'draft'::public.return_invoice_status NOT NULL,
    notes text,
    total_amount numeric(12,2) DEFAULT 0.00,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL,
    is_fiscal boolean NOT NULL,
    return_action public.return_action_type NOT NULL,
    exchange_invoice_id uuid,
    source_invoice_id uuid,
    created_by_id uuid NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.stock (
    store_id uuid NOT NULL,
    product_id uuid NOT NULL,
    quantity numeric(10,3) DEFAULT 0.000 NOT NULL,
    price numeric(10,2) DEFAULT 0.00 NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL
);

CREATE TABLE IF NOT EXISTS public.stores (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    name character varying(255) NOT NULL,
    address character varying(500),
    phone character varying(50),
    legal_name character varying(255),
    edrpou character varying(20),
    is_active boolean DEFAULT true NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL
);

CREATE TABLE IF NOT EXISTS public.supplier_ledger (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    supplier_id uuid NOT NULL,
    operation_type public.ledger_operation_type NOT NULL,
    document_id uuid,
    document_number character varying(50),
    amount numeric(12,2) NOT NULL,
    balance_after numeric(12,2) NOT NULL,
    operation_date timestamp without time zone NOT NULL,
    notes text,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.suppliers (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    name character varying(255) NOT NULL,
    edrpou character varying(10),
    phone character varying(20),
    email character varying(255),
    address text,
    notes text,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL
);

CREATE TABLE IF NOT EXISTS public.system_settings (
    id uuid NOT NULL,
    module character varying(50) NOT NULL,
    key character varying(100) NOT NULL,
    value text,
    value_type character varying(20) NOT NULL,
    label character varying(255) NOT NULL,
    description text,
    options text,
    is_active boolean NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.transfer_items (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    transfer_id uuid NOT NULL,
    product_id uuid NOT NULL,
    quantity numeric(10,3) NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    cost_price numeric(12,2) DEFAULT '0'::numeric NOT NULL,
    price numeric(12,2) DEFAULT '0'::numeric NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.transfers (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    number character varying(50) NOT NULL,
    from_location character varying(255) NOT NULL,
    to_location character varying(255) NOT NULL,
    transfer_date timestamp without time zone NOT NULL,
    status public.transfer_status DEFAULT 'draft'::public.transfer_status NOT NULL,
    notes text,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL,
    created_by_id uuid NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.user_stores (
    user_id uuid NOT NULL,
    store_id uuid NOT NULL,
    role character varying(16) DEFAULT 'cashier'::character varying NOT NULL,
    permissions jsonb DEFAULT '{}'::jsonb NOT NULL,
    is_default boolean DEFAULT false NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL
);

CREATE TABLE IF NOT EXISTS public.users (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    name character varying(255) NOT NULL,
    login character varying(100) NOT NULL,
    password_hash character varying(255) NOT NULL,
    pin_code character varying(255),
    role public.user_role DEFAULT 'cashier'::public.user_role NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL,
    permissions jsonb,
    hourly_rate numeric(10,2),
    last_login_at timestamp without time zone,
    onboarding_completed boolean DEFAULT true NOT NULL
);

CREATE TABLE IF NOT EXISTS public.work_sessions (
    id uuid NOT NULL,
    user_id uuid NOT NULL,
    login_time timestamp without time zone NOT NULL,
    logout_time timestamp without time zone,
    duration_hours numeric(5,2),
    created_at timestamp without time zone NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.write_off_items (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    write_off_id uuid NOT NULL,
    product_id uuid NOT NULL,
    quantity numeric(10,3) NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    cost_price numeric(12,2) DEFAULT '0'::numeric NOT NULL,
    price numeric(12,2) DEFAULT '0'::numeric NOT NULL,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.write_off_reasons (
    id uuid NOT NULL,
    name character varying(100) NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL
);

CREATE TABLE IF NOT EXISTS public.write_offs (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    number character varying(50) NOT NULL,
    reason character varying(100) NOT NULL,
    write_off_date timestamp without time zone NOT NULL,
    notes text,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL,
    status character varying(20) DEFAULT 'confirmed'::character varying NOT NULL,
    total_amount numeric(12,2) DEFAULT 0.00,
    created_by_id uuid NOT NULL,
    custom_reason text,
    store_id uuid
);

CREATE TABLE IF NOT EXISTS public.cash_operations (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    store_id uuid NOT NULL,
    user_id uuid NOT NULL,
    operation_type character varying(16) NOT NULL,
    cash_type character varying(8) DEFAULT 'cash'::character varying NOT NULL,
    amount numeric(12,2) NOT NULL,
    comment text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT cash_operations_operation_type_check CHECK ((operation_type = ANY (ARRAY['deposit'::text, 'collection'::text]))),
    CONSTRAINT cash_operations_cash_type_check CHECK ((cash_type = ANY (ARRAY['cash'::text, 'card'::text]))),
    CONSTRAINT cash_operations_amount_check CHECK ((amount > (0)::numeric))
);

ALTER TABLE ONLY public.prro_settings ALTER COLUMN id SET DEFAULT nextval('public.prro_settings_id_seq'::regclass);

ALTER TABLE ONLY public.barcodes
    ADD CONSTRAINT barcodes_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.categories
    ADD CONSTRAINT categories_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.cash_operations
    ADD CONSTRAINT cash_operations_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.debtor_payments
    ADD CONSTRAINT debtor_payments_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.debtors
    ADD CONSTRAINT debtors_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.inventories
    ADD CONSTRAINT inventories_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.inventory_items
    ADD CONSTRAINT inventory_items_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.invoice_items
    ADD CONSTRAINT invoice_items_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.invoices
    ADD CONSTRAINT invoices_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.print_templates
    ADD CONSTRAINT print_templates_pkey PRIMARY KEY (id);


ALTER TABLE ONLY public.product_images
    ADD CONSTRAINT product_images_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.products
    ADD CONSTRAINT products_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.prro_queue_items
    ADD CONSTRAINT prro_queue_items_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.prro_settings
    ADD CONSTRAINT prro_settings_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.prro_shifts
    ADD CONSTRAINT prro_shifts_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.purchase_order_items
    ADD CONSTRAINT purchase_order_items_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.purchase_orders
    ADD CONSTRAINT purchase_orders_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.receipt_items
    ADD CONSTRAINT receipt_items_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.receipts
    ADD CONSTRAINT receipts_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.return_invoice_items
    ADD CONSTRAINT return_invoice_items_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.return_invoices
    ADD CONSTRAINT return_invoices_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.stock
    ADD CONSTRAINT stock_pkey PRIMARY KEY (store_id, product_id);

ALTER TABLE ONLY public.stores
    ADD CONSTRAINT stores_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.supplier_ledger
    ADD CONSTRAINT supplier_ledger_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.suppliers
    ADD CONSTRAINT suppliers_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.system_settings
    ADD CONSTRAINT system_settings_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.transfer_items
    ADD CONSTRAINT transfer_items_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.transfers
    ADD CONSTRAINT transfers_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.user_stores
    ADD CONSTRAINT user_stores_pkey PRIMARY KEY (user_id, store_id);

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.work_sessions
    ADD CONSTRAINT work_sessions_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.write_off_items
    ADD CONSTRAINT write_off_items_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.write_off_reasons
    ADD CONSTRAINT write_off_reasons_name_key UNIQUE (name);

ALTER TABLE ONLY public.write_off_reasons
    ADD CONSTRAINT write_off_reasons_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.write_offs
    ADD CONSTRAINT write_offs_pkey PRIMARY KEY (id);

CREATE UNIQUE INDEX ix_barcodes_barcode ON public.barcodes USING btree (barcode);

CREATE INDEX ix_barcodes_product_id ON public.barcodes USING btree (product_id);

CREATE INDEX ix_barcodes_store_created ON public.barcodes USING btree (store_id, created_at);

CREATE INDEX ix_categories_name ON public.categories USING btree (name);

CREATE INDEX ix_categories_parent_id ON public.categories USING btree (parent_id);

CREATE INDEX ix_categories_store_created ON public.categories USING btree (store_id, created_at);

CREATE INDEX ix_debtor_payments_debtor_id ON public.debtor_payments USING btree (debtor_id);

CREATE INDEX ix_debtor_payments_store_created ON public.debtor_payments USING btree (store_id, created_at);

CREATE INDEX ix_debtors_name ON public.debtors USING btree (name);

CREATE INDEX ix_debtors_store_created ON public.debtors USING btree (store_id, created_at);

CREATE INDEX ix_inventories_number ON public.inventories USING btree (number);

CREATE INDEX ix_inventories_store_created ON public.inventories USING btree (store_id, created_at);

CREATE INDEX ix_inventory_items_inventory_id ON public.inventory_items USING btree (inventory_id);

CREATE INDEX ix_inventory_items_product_id ON public.inventory_items USING btree (product_id);

CREATE INDEX ix_inventory_items_store_created ON public.inventory_items USING btree (store_id, created_at);

CREATE INDEX ix_invoice_items_invoice_id ON public.invoice_items USING btree (invoice_id);

CREATE INDEX ix_invoice_items_product_id ON public.invoice_items USING btree (product_id);

CREATE INDEX ix_cash_operations_store_id ON public.cash_operations USING btree (store_id);

CREATE INDEX ix_invoice_items_store_created ON public.invoice_items USING btree (store_id, created_at);

CREATE INDEX ix_invoices_number ON public.invoices USING btree (number);

CREATE INDEX ix_invoices_store_created ON public.invoices USING btree (store_id, created_at);

CREATE INDEX ix_invoices_supplier_id ON public.invoices USING btree (supplier_id);

CREATE INDEX ix_print_templates_type ON public.print_templates USING btree (type);

CREATE INDEX ix_product_images_product_id ON public.product_images USING btree (product_id);

CREATE INDEX ix_product_images_store_created ON public.product_images USING btree (store_id, created_at);

CREATE UNIQUE INDEX ix_products_barcode ON public.products USING btree (barcode);

CREATE INDEX ix_products_category_id ON public.products USING btree (category_id);

CREATE UNIQUE INDEX ix_products_sku ON public.products USING btree (sku);

CREATE INDEX ix_products_supplier_id ON public.products USING btree (supplier_id);

CREATE INDEX ix_products_title ON public.products USING btree (title);

CREATE INDEX ix_prro_queue_items_receipt_id ON public.prro_queue_items USING btree (receipt_id);

CREATE INDEX ix_prro_queue_items_shift_id ON public.prro_queue_items USING btree (shift_id);

CREATE INDEX ix_prro_queue_items_status ON public.prro_queue_items USING btree (status);

CREATE INDEX ix_prro_queue_items_store_status ON public.prro_queue_items USING btree (store_id, status, created_at);

CREATE UNIQUE INDEX ux_prro_settings_store_key ON public.prro_settings USING btree (store_id, key_name);

CREATE INDEX ix_prro_shifts_shift_number ON public.prro_shifts USING btree (shift_number);

CREATE INDEX ix_prro_shifts_store_opened ON public.prro_shifts USING btree (store_id, opened_at);

CREATE INDEX ix_purchase_order_items_product_id ON public.purchase_order_items USING btree (product_id);

CREATE INDEX ix_purchase_order_items_purchase_order_id ON public.purchase_order_items USING btree (purchase_order_id);

CREATE INDEX ix_purchase_order_items_store_created ON public.purchase_order_items USING btree (store_id, created_at);

CREATE INDEX ix_purchase_orders_invoice_id ON public.purchase_orders USING btree (invoice_id);

CREATE INDEX ix_purchase_orders_number ON public.purchase_orders USING btree (number);

CREATE INDEX ix_purchase_orders_store_created ON public.purchase_orders USING btree (store_id, created_at);

CREATE INDEX ix_purchase_orders_supplier_id ON public.purchase_orders USING btree (supplier_id);

CREATE INDEX ix_receipt_items_product_id ON public.receipt_items USING btree (product_id);

CREATE INDEX ix_receipt_items_receipt_id ON public.receipt_items USING btree (receipt_id);

CREATE INDEX ix_receipt_items_store_created ON public.receipt_items USING btree (store_id, created_at);

CREATE INDEX ix_receipts_cashier_id ON public.receipts USING btree (cashier_id);

CREATE INDEX ix_receipts_debtor_id ON public.receipts USING btree (debtor_id);

CREATE INDEX ix_receipts_original_receipt_id ON public.receipts USING btree (original_receipt_id);

CREATE INDEX ix_receipts_receipt_number ON public.receipts USING btree (receipt_number);

CREATE INDEX ix_receipts_split_group_id ON public.receipts USING btree (split_group_id);

CREATE INDEX ix_receipts_store_created ON public.receipts USING btree (store_id, created_at);

CREATE INDEX ix_return_invoice_items_product_id ON public.return_invoice_items USING btree (product_id);

CREATE INDEX ix_return_invoice_items_return_invoice_id ON public.return_invoice_items USING btree (return_invoice_id);

CREATE INDEX ix_return_invoice_items_store_created ON public.return_invoice_items USING btree (store_id, created_at);

CREATE INDEX ix_return_invoices_exchange_invoice_id ON public.return_invoices USING btree (exchange_invoice_id);

CREATE INDEX ix_return_invoices_number ON public.return_invoices USING btree (number);

CREATE INDEX ix_return_invoices_source_invoice_id ON public.return_invoices USING btree (source_invoice_id);

CREATE INDEX ix_return_invoices_store_created ON public.return_invoices USING btree (store_id, created_at);

CREATE INDEX ix_return_invoices_supplier_id ON public.return_invoices USING btree (supplier_id);

CREATE INDEX ix_stock_product ON public.stock USING btree (product_id);

CREATE INDEX ix_stores_name ON public.stores USING btree (name);

CREATE INDEX ix_supplier_ledger_store_created ON public.supplier_ledger USING btree (store_id, created_at);

CREATE INDEX ix_supplier_ledger_supplier_id ON public.supplier_ledger USING btree (supplier_id);

CREATE INDEX ix_suppliers_name ON public.suppliers USING btree (name);

CREATE UNIQUE INDEX ix_system_settings_key ON public.system_settings USING btree (store_id, key);

CREATE INDEX ix_system_settings_module ON public.system_settings USING btree (module);

CREATE INDEX ix_system_settings_store_created ON public.system_settings USING btree (store_id, created_at);

CREATE INDEX ix_transfer_items_product_id ON public.transfer_items USING btree (product_id);

CREATE INDEX ix_transfer_items_store_created ON public.transfer_items USING btree (store_id, created_at);

CREATE INDEX ix_transfer_items_transfer_id ON public.transfer_items USING btree (transfer_id);

CREATE INDEX ix_transfers_number ON public.transfers USING btree (number);

CREATE INDEX ix_transfers_store_created ON public.transfers USING btree (store_id, created_at);

CREATE INDEX ix_user_stores_role ON public.user_stores USING btree (role);

CREATE INDEX ix_user_stores_store ON public.user_stores USING btree (store_id);

CREATE UNIQUE INDEX ix_users_login ON public.users USING btree (login);

CREATE INDEX ix_work_sessions_store_created ON public.work_sessions USING btree (store_id, created_at);

CREATE INDEX ix_work_sessions_user_id ON public.work_sessions USING btree (user_id);

CREATE INDEX ix_write_off_items_product_id ON public.write_off_items USING btree (product_id);

CREATE INDEX ix_write_off_items_store_created ON public.write_off_items USING btree (store_id, created_at);

CREATE INDEX ix_write_off_items_write_off_id ON public.write_off_items USING btree (write_off_id);

CREATE INDEX ix_write_offs_number ON public.write_offs USING btree (number);

CREATE INDEX ix_write_offs_store_created ON public.write_offs USING btree (store_id, created_at);

CREATE UNIQUE INDEX uq_print_templates_default_per_type ON public.print_templates USING btree (type, store_id) WHERE (is_default = true);

ALTER TABLE ONLY public.barcodes
    ADD CONSTRAINT barcodes_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.categories
    ADD CONSTRAINT categories_parent_id_fkey FOREIGN KEY (parent_id) REFERENCES public.categories(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.debtor_payments
    ADD CONSTRAINT debtor_payments_debtor_id_fkey FOREIGN KEY (debtor_id) REFERENCES public.debtors(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.barcodes
    ADD CONSTRAINT fk_barcodes_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.categories
    ADD CONSTRAINT fk_categories_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.debtor_payments
    ADD CONSTRAINT fk_debtor_payments_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.debtors
    ADD CONSTRAINT fk_debtors_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.inventories
    ADD CONSTRAINT fk_inventories_created_by_id FOREIGN KEY (created_by_id) REFERENCES public.users(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.inventories
    ADD CONSTRAINT fk_inventories_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.inventory_items
    ADD CONSTRAINT fk_inventory_items_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.invoice_items
    ADD CONSTRAINT fk_invoice_items_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.invoices
    ADD CONSTRAINT fk_invoices_created_by_id FOREIGN KEY (created_by_id) REFERENCES public.users(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.invoices
    ADD CONSTRAINT fk_invoices_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.product_images
    ADD CONSTRAINT fk_product_images_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.purchase_order_items
    ADD CONSTRAINT fk_purchase_order_items_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.purchase_orders
    ADD CONSTRAINT fk_purchase_orders_created_by_id FOREIGN KEY (created_by_id) REFERENCES public.users(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.purchase_orders
    ADD CONSTRAINT fk_purchase_orders_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.receipt_items
    ADD CONSTRAINT fk_receipt_items_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.receipts
    ADD CONSTRAINT fk_receipts_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.return_invoice_items
    ADD CONSTRAINT fk_return_invoice_items_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.return_invoices
    ADD CONSTRAINT fk_return_invoices_created_by_id FOREIGN KEY (created_by_id) REFERENCES public.users(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.return_invoices
    ADD CONSTRAINT fk_return_invoices_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.supplier_ledger
    ADD CONSTRAINT fk_supplier_ledger_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.system_settings
    ADD CONSTRAINT fk_system_settings_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.transfer_items
    ADD CONSTRAINT fk_transfer_items_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.transfers
    ADD CONSTRAINT fk_transfers_created_by_id FOREIGN KEY (created_by_id) REFERENCES public.users(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.transfers
    ADD CONSTRAINT fk_transfers_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.work_sessions
    ADD CONSTRAINT fk_work_sessions_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.write_off_items
    ADD CONSTRAINT fk_write_off_items_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.write_offs
    ADD CONSTRAINT fk_write_offs_created_by_id FOREIGN KEY (created_by_id) REFERENCES public.users(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.write_offs
    ADD CONSTRAINT fk_write_offs_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.inventory_items
    ADD CONSTRAINT inventory_items_inventory_id_fkey FOREIGN KEY (inventory_id) REFERENCES public.inventories(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.inventory_items
    ADD CONSTRAINT inventory_items_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.invoice_items
    ADD CONSTRAINT invoice_items_invoice_id_fkey FOREIGN KEY (invoice_id) REFERENCES public.invoices(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.invoice_items
    ADD CONSTRAINT invoice_items_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.invoices
    ADD CONSTRAINT invoices_supplier_id_fkey FOREIGN KEY (supplier_id) REFERENCES public.suppliers(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.product_images
    ADD CONSTRAINT product_images_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.products
    ADD CONSTRAINT products_category_id_fkey FOREIGN KEY (category_id) REFERENCES public.categories(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.products
    ADD CONSTRAINT products_supplier_id_fkey FOREIGN KEY (supplier_id) REFERENCES public.suppliers(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.prro_queue_items
    ADD CONSTRAINT prro_queue_items_receipt_id_fkey FOREIGN KEY (receipt_id) REFERENCES public.receipts(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.prro_queue_items
    ADD CONSTRAINT prro_queue_items_shift_id_fkey FOREIGN KEY (shift_id) REFERENCES public.prro_shifts(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.prro_settings
    ADD CONSTRAINT prro_settings_store_id_fkey FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.prro_shifts
    ADD CONSTRAINT prro_shifts_store_id_fkey FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.prro_queue_items
    ADD CONSTRAINT prro_queue_items_store_id_fkey FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.purchase_order_items
    ADD CONSTRAINT purchase_order_items_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.purchase_order_items
    ADD CONSTRAINT purchase_order_items_purchase_order_id_fkey FOREIGN KEY (purchase_order_id) REFERENCES public.purchase_orders(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.purchase_orders
    ADD CONSTRAINT purchase_orders_invoice_id_fkey FOREIGN KEY (invoice_id) REFERENCES public.invoices(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.purchase_orders
    ADD CONSTRAINT purchase_orders_supplier_id_fkey FOREIGN KEY (supplier_id) REFERENCES public.suppliers(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.receipt_items
    ADD CONSTRAINT receipt_items_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.receipt_items
    ADD CONSTRAINT receipt_items_receipt_id_fkey FOREIGN KEY (receipt_id) REFERENCES public.receipts(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.receipts
    ADD CONSTRAINT receipts_cashier_id_fkey FOREIGN KEY (cashier_id) REFERENCES public.users(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.receipts
    ADD CONSTRAINT receipts_debtor_id_fkey FOREIGN KEY (debtor_id) REFERENCES public.debtors(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.receipts
    ADD CONSTRAINT receipts_original_receipt_id_fkey FOREIGN KEY (original_receipt_id) REFERENCES public.receipts(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.receipts
    ADD CONSTRAINT receipts_split_group_id_fkey FOREIGN KEY (split_group_id) REFERENCES public.receipts(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.return_invoice_items
    ADD CONSTRAINT return_invoice_items_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.return_invoice_items
    ADD CONSTRAINT return_invoice_items_return_invoice_id_fkey FOREIGN KEY (return_invoice_id) REFERENCES public.return_invoices(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.return_invoices
    ADD CONSTRAINT return_invoices_exchange_invoice_id_fkey FOREIGN KEY (exchange_invoice_id) REFERENCES public.invoices(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.return_invoices
    ADD CONSTRAINT return_invoices_source_invoice_id_fkey FOREIGN KEY (source_invoice_id) REFERENCES public.invoices(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.return_invoices
    ADD CONSTRAINT return_invoices_supplier_id_fkey FOREIGN KEY (supplier_id) REFERENCES public.suppliers(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.stock
    ADD CONSTRAINT stock_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.stock
    ADD CONSTRAINT stock_store_id_fkey FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.supplier_ledger
    ADD CONSTRAINT supplier_ledger_supplier_id_fkey FOREIGN KEY (supplier_id) REFERENCES public.suppliers(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.transfer_items
    ADD CONSTRAINT transfer_items_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.transfer_items
    ADD CONSTRAINT transfer_items_transfer_id_fkey FOREIGN KEY (transfer_id) REFERENCES public.transfers(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.user_stores
    ADD CONSTRAINT user_stores_store_id_fkey FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.user_stores
    ADD CONSTRAINT user_stores_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.work_sessions
    ADD CONSTRAINT work_sessions_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.write_off_items
    ADD CONSTRAINT write_off_items_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE RESTRICT;

ALTER TABLE ONLY public.write_off_items
    ADD CONSTRAINT write_off_items_write_off_id_fkey FOREIGN KEY (write_off_id) REFERENCES public.write_offs(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.print_templates
    ADD CONSTRAINT fk_print_templates_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;
ALTER TABLE public.barcodes ENABLE ROW LEVEL SECURITY;

CREATE POLICY barcodes_store_isolation ON public.barcodes USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.categories ENABLE ROW LEVEL SECURITY;

CREATE POLICY categories_store_isolation ON public.categories USING (((store_id IS NULL) OR (store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id IS NULL) OR (store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.debtor_payments ENABLE ROW LEVEL SECURITY;

CREATE POLICY debtor_payments_store_isolation ON public.debtor_payments USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.debtors ENABLE ROW LEVEL SECURITY;

CREATE POLICY debtors_store_isolation ON public.debtors USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.inventories ENABLE ROW LEVEL SECURITY;

CREATE POLICY inventories_store_isolation ON public.inventories USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.inventory_items ENABLE ROW LEVEL SECURITY;

CREATE POLICY inventory_items_store_isolation ON public.inventory_items USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.invoice_items ENABLE ROW LEVEL SECURITY;

CREATE POLICY invoice_items_store_isolation ON public.invoice_items USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.invoices ENABLE ROW LEVEL SECURITY;

CREATE POLICY invoices_store_isolation ON public.invoices USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.product_images ENABLE ROW LEVEL SECURITY;

CREATE POLICY product_images_store_isolation ON public.product_images USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.purchase_order_items ENABLE ROW LEVEL SECURITY;

CREATE POLICY purchase_order_items_store_isolation ON public.purchase_order_items USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.purchase_orders ENABLE ROW LEVEL SECURITY;

CREATE POLICY purchase_orders_store_isolation ON public.purchase_orders USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.receipt_items ENABLE ROW LEVEL SECURITY;

CREATE POLICY receipt_items_store_isolation ON public.receipt_items USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.receipts ENABLE ROW LEVEL SECURITY;

CREATE POLICY receipts_store_isolation ON public.receipts USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.return_invoice_items ENABLE ROW LEVEL SECURITY;

CREATE POLICY return_invoice_items_store_isolation ON public.return_invoice_items USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.return_invoices ENABLE ROW LEVEL SECURITY;

CREATE POLICY return_invoices_store_isolation ON public.return_invoices USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.stock ENABLE ROW LEVEL SECURITY;

CREATE POLICY stock_store_isolation ON public.stock USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.stores ENABLE ROW LEVEL SECURITY;

CREATE POLICY stores_access ON public.stores USING ((id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))));

ALTER TABLE public.supplier_ledger ENABLE ROW LEVEL SECURITY;

CREATE POLICY supplier_ledger_store_isolation ON public.supplier_ledger USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.system_settings ENABLE ROW LEVEL SECURITY;

CREATE POLICY system_settings_store_isolation ON public.system_settings USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.transfer_items ENABLE ROW LEVEL SECURITY;

CREATE POLICY transfer_items_store_isolation ON public.transfer_items USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.transfers ENABLE ROW LEVEL SECURITY;

CREATE POLICY transfers_store_isolation ON public.transfers USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.user_stores ENABLE ROW LEVEL SECURITY;

CREATE POLICY user_stores_self ON public.user_stores USING ((user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid));

ALTER TABLE public.work_sessions ENABLE ROW LEVEL SECURITY;

CREATE POLICY work_sessions_store_isolation ON public.work_sessions USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.write_off_items ENABLE ROW LEVEL SECURITY;

CREATE POLICY write_off_items_store_isolation ON public.write_off_items USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.write_offs ENABLE ROW LEVEL SECURITY;

CREATE POLICY write_offs_store_isolation ON public.write_offs USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));



ALTER TABLE public.prro_settings ENABLE ROW LEVEL SECURITY;

CREATE POLICY prro_settings_store_isolation ON public.prro_settings USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.prro_shifts ENABLE ROW LEVEL SECURITY;

CREATE POLICY prro_shifts_store_isolation ON public.prro_shifts USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));

ALTER TABLE public.prro_queue_items ENABLE ROW LEVEL SECURITY;

CREATE POLICY prro_queue_items_store_isolation ON public.prro_queue_items USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


-- ============================================================================
-- owners_db — мета-таблиця: персональна БД кожного власника (Частина 2)
-- ============================================================================
CREATE TABLE IF NOT EXISTS public.owners_db (
    owner_id uuid NOT NULL,
    db_name text NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    CONSTRAINT owners_db_pkey PRIMARY KEY (owner_id),
    CONSTRAINT owners_db_db_name_key UNIQUE (db_name),
    CONSTRAINT owners_db_owner_id_fkey FOREIGN KEY (owner_id)
        REFERENCES public.users(id) ON DELETE CASCADE
);

-- ============================================================================
-- Мережевий рівень власника (Частина 3): devices, store_activation_codes,
-- store_product_prices, audit_log, store_sync_state.
-- Створюються ідемпотентно: fresh (schema.sql) і вже мігровані БД (NETWORK_DDL).
-- ============================================================================

-- device_status: PG не має CREATE TYPE IF NOT EXISTS — через DO-блок.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'device_status') THEN
        CREATE TYPE public.device_status AS ENUM ('pending', 'active', 'blocked', 'deleted');
    END IF;
END
$$;

-- Фізичні каси/пристрої мережі. store_id → точка; status — життєвий цикл
-- пристрою (pending → active → blocked/deleted). device_token_hash — токен
-- аутентифікації пристрою (зберігається лише хеш).
CREATE TABLE IF NOT EXISTS public.devices (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    store_id uuid NOT NULL,
    name character varying(255) NOT NULL,
    device_token_hash character varying(255) NOT NULL,
    source character varying(50),
    status public.device_status DEFAULT 'pending'::public.device_status NOT NULL,
    app_version character varying(50),
    last_seen_at timestamp without time zone,
    activated_at timestamp without time zone,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL,
    CONSTRAINT devices_pkey PRIMARY KEY (id),
    CONSTRAINT devices_store_id_fkey FOREIGN KEY (store_id)
        REFERENCES public.stores(id) ON DELETE CASCADE
);

-- Код активації точки (8 символів; колонка varchar(9) — резерв під префікс).
-- Один рядок на магазин: повторна генерація оновлює код (regenerated_at).
CREATE TABLE IF NOT EXISTS public.store_activation_codes (
    store_id uuid NOT NULL,
    code character varying(9) NOT NULL,
    created_by uuid,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    regenerated_at timestamp without time zone,
    CONSTRAINT store_activation_codes_pkey PRIMARY KEY (store_id),
    CONSTRAINT store_activation_codes_code_key UNIQUE (code),
    CONSTRAINT store_activation_codes_created_by_fkey FOREIGN KEY (created_by)
        REFERENCES public.users(id) ON DELETE SET NULL,
    CONSTRAINT store_activation_codes_store_id_fkey FOREIGN KEY (store_id)
        REFERENCES public.stores(id) ON DELETE CASCADE
);

-- Перевизначення ціни товару по конкретній точці (owner-рівень; на відміну
-- від stock.price — окрема сутність, не залежить від залишків).
CREATE TABLE IF NOT EXISTS public.store_product_prices (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    store_id uuid NOT NULL,
    product_id uuid NOT NULL,
    price numeric(10,2) NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL,
    CONSTRAINT store_product_prices_pkey PRIMARY KEY (id),
    CONSTRAINT store_product_prices_store_id_product_id_key UNIQUE (store_id, product_id),
    CONSTRAINT store_product_prices_product_id_fkey FOREIGN KEY (product_id)
        REFERENCES public.products(id) ON DELETE CASCADE,
    CONSTRAINT store_product_prices_store_id_fkey FOREIGN KEY (store_id)
        REFERENCES public.stores(id) ON DELETE CASCADE
);

-- Аудит дій адмінки (хто/що/коли; payload — контекст дії в JSON).
-- FK з ON DELETE SET NULL: аудит-слід зберігається при видаленні юзера/точки.
CREATE TABLE IF NOT EXISTS public.audit_log (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    actor_user_id uuid,
    action character varying(100) NOT NULL,
    entity_type character varying(50),
    entity_id uuid,
    store_id uuid,
    payload jsonb,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    CONSTRAINT audit_log_pkey PRIMARY KEY (id),
    CONSTRAINT audit_log_actor_user_id_fkey FOREIGN KEY (actor_user_id)
        REFERENCES public.users(id) ON DELETE SET NULL,
    CONSTRAINT audit_log_store_id_fkey FOREIGN KEY (store_id)
        REFERENCES public.stores(id) ON DELETE SET NULL
);

-- Стан синхронізації точки (офлайн-first): час останньої синхронізації та
-- останній локальний seq, до якого сервер отримав дані точки.
CREATE TABLE IF NOT EXISTS public.store_sync_state (
    store_id uuid NOT NULL,
    device_id uuid,
    last_synced_at timestamp without time zone,
    last_local_seq bigint DEFAULT 0 NOT NULL,
    status character varying(20) DEFAULT 'unknown'::character varying NOT NULL,
    CONSTRAINT store_sync_state_pkey PRIMARY KEY (store_id),
    CONSTRAINT store_sync_state_device_id_fkey FOREIGN KEY (device_id)
        REFERENCES public.devices(id) ON DELETE SET NULL,
    CONSTRAINT store_sync_state_store_id_fkey FOREIGN KEY (store_id)
        REFERENCES public.stores(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS ix_devices_store_id ON public.devices USING btree (store_id);

-- Не більше одного legacy-пристрою (source='legacy_migration') на точку:
-- ідемпотентність POST /admin/migrate/legacy на рівні БД (race-safe).
CREATE UNIQUE INDEX IF NOT EXISTS ux_devices_legacy_migration_store
    ON public.devices (store_id) WHERE (source = 'legacy_migration');

CREATE INDEX IF NOT EXISTS ix_store_product_prices_product_id
    ON public.store_product_prices USING btree (product_id);

CREATE INDEX IF NOT EXISTS ix_audit_log_store_id_created_at
    ON public.audit_log USING btree (store_id, created_at);
