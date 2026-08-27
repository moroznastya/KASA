--
-- PostgreSQL database dump
--


-- Dumped from database version 16.14 (Ubuntu 16.14-0ubuntu0.24.04.1)
-- Dumped by pg_dump version 17.6 (Ubuntu 17.6-1.pgdg24.04+1)

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: dblink; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS dblink WITH SCHEMA public;


--
-- Name: EXTENSION dblink; Type: COMMENT; Schema: -; Owner: -
--

COMMENT ON EXTENSION dblink IS 'connect to other PostgreSQL databases from within a database';


--
-- Name: pg_trgm; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS pg_trgm WITH SCHEMA public;


--
-- Name: EXTENSION pg_trgm; Type: COMMENT; Schema: -; Owner: -
--

COMMENT ON EXTENSION pg_trgm IS 'text similarity measurement and index searching based on trigrams';


--
-- Name: uuid-ossp; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS "uuid-ossp" WITH SCHEMA public;


--
-- Name: EXTENSION "uuid-ossp"; Type: COMMENT; Schema: -; Owner: -
--

COMMENT ON EXTENSION "uuid-ossp" IS 'generate universally unique identifiers (UUIDs)';


--
-- Name: enum_users_role; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.enum_users_role AS ENUM (
    'admin',
    'cashier'
);


--
-- Name: fiscal_status; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.fiscal_status AS ENUM (
    'none',
    'pending',
    'sent',
    'failed',
    'fiscalized'
);


--
-- Name: inventory_status; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.inventory_status AS ENUM (
    'draft',
    'confirmed',
    'cancelled'
);


--
-- Name: invoice_status; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.invoice_status AS ENUM (
    'draft',
    'confirmed',
    'cancelled'
);


--
-- Name: ledger_operation_type; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.ledger_operation_type AS ENUM (
    'invoice',
    'payment',
    'return',
    'correction'
);


--
-- Name: payment_method; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.payment_method AS ENUM (
    'credit',
    'bank_transfer',
    'cash',
    'other'
);


--
-- Name: prro_queue_status; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.prro_queue_status AS ENUM (
    'pending',
    'sent',
    'failed'
);


--
-- Name: prro_shift_status; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.prro_shift_status AS ENUM (
    'open',
    'closed'
);


--
-- Name: purchase_order_status; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.purchase_order_status AS ENUM (
    'draft',
    'confirmed',
    'cancelled'
);


--
-- Name: receipt_payment_method; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.receipt_payment_method AS ENUM (
    'cash',
    'card',
    'mixed'
);


--
-- Name: receipt_type; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.receipt_type AS ENUM (
    'sale',
    'return'
);


--
-- Name: return_action_type; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.return_action_type AS ENUM (
    'deduct_from_debt',
    'add_to_cash',
    'exchange'
);


--
-- Name: return_invoice_status; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.return_invoice_status AS ENUM (
    'draft',
    'confirmed',
    'cancelled'
);


--
-- Name: transfer_status; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.transfer_status AS ENUM (
    'draft',
    'confirmed',
    'cancelled'
);


--
-- Name: user_role; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.user_role AS ENUM (
    'admin',
    'cashier',
    'owner'
);


SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: barcodes; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.barcodes (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    product_id uuid NOT NULL,
    barcode character varying(50) NOT NULL,
    is_primary boolean DEFAULT false NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL,
    store_id uuid
);


--
-- Name: cash_operations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.cash_operations (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    store_id uuid NOT NULL,
    user_id uuid NOT NULL,
    operation_type character varying(16) NOT NULL,
    amount numeric(12,2) NOT NULL,
    comment text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    cash_type character varying(8) DEFAULT 'cash'::character varying NOT NULL,
    CONSTRAINT cash_operations_amount_check CHECK ((amount > (0)::numeric)),
    CONSTRAINT cash_operations_cash_type_check CHECK (((cash_type)::text = ANY (ARRAY['cash'::text, 'card'::text]))),
    CONSTRAINT cash_operations_operation_type_check CHECK (((operation_type)::text = ANY ((ARRAY['deposit'::character varying, 'collection'::character varying])::text[])))
);


--
-- Name: categories; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.categories (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    name character varying(255) NOT NULL,
    description text,
    parent_id uuid,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL,
    store_id uuid
);


--
-- Name: debtor_payments; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.debtor_payments (
    id uuid NOT NULL,
    debtor_id uuid NOT NULL,
    amount numeric(12,2) NOT NULL,
    payment_method character varying(20),
    created_at timestamp without time zone NOT NULL,
    store_id uuid
);


--
-- Name: debtors; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.debtors (
    id uuid NOT NULL,
    name character varying(255) NOT NULL,
    phone character varying(50),
    notes text,
    total_debt numeric(12,2) NOT NULL,
    created_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    store_id uuid
);


--
-- Name: inventories; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.inventories (
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


--
-- Name: inventory_items; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.inventory_items (
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


--
-- Name: invoice_items; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.invoice_items (
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


--
-- Name: invoices; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.invoices (
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


--
-- Name: owners_db; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.owners_db (
    owner_id uuid NOT NULL,
    db_name text NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL
);


--
-- Name: print_templates; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.print_templates (
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


--
-- Name: product_images; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.product_images (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    product_id uuid NOT NULL,
    url character varying(1024) NOT NULL,
    is_main boolean DEFAULT false NOT NULL,
    sort_order integer DEFAULT 0 NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL,
    store_id uuid
);


--
-- Name: products; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.products (
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


--
-- Name: prro_queue_items; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.prro_queue_items (
    id uuid NOT NULL,
    receipt_id uuid,
    shift_id uuid,
    local_number integer NOT NULL,
    check_type character varying(10) NOT NULL,
    xml_body text NOT NULL,
    mac text,
    status public.prro_queue_status NOT NULL,
    error text,
    created_at timestamp without time zone NOT NULL,
    sent_at timestamp without time zone,
    check_sign text,
    id_offline text
);


--
-- Name: prro_settings; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.prro_settings (
    id integer NOT NULL,
    key_name character varying(100) NOT NULL,
    value text,
    updated_at timestamp without time zone NOT NULL
);


--
-- Name: prro_settings_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.prro_settings_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: prro_settings_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.prro_settings_id_seq OWNED BY public.prro_settings.id;


--
-- Name: prro_shifts; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.prro_shifts (
    id uuid NOT NULL,
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


--
-- Name: purchase_order_items; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.purchase_order_items (
    id uuid NOT NULL,
    purchase_order_id uuid NOT NULL,
    product_id uuid NOT NULL,
    quantity numeric(10,3) NOT NULL,
    price numeric(10,2) NOT NULL,
    total numeric(12,2) NOT NULL,
    created_at timestamp without time zone NOT NULL,
    store_id uuid
);


--
-- Name: purchase_orders; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.purchase_orders (
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


--
-- Name: receipt_items; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.receipt_items (
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


--
-- Name: receipts; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.receipts (
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


--
-- Name: return_invoice_items; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.return_invoice_items (
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


--
-- Name: return_invoices; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.return_invoices (
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


--
-- Name: stock; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.stock (
    store_id uuid NOT NULL,
    product_id uuid NOT NULL,
    quantity numeric(10,3) DEFAULT 0.000 NOT NULL,
    price numeric(10,2) DEFAULT 0.00 NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL
);


--
-- Name: stores; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.stores (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    name character varying(255) NOT NULL,
    address character varying(500),
    phone character varying(50),
    is_active boolean DEFAULT true NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL
);


--
-- Name: supplier_ledger; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.supplier_ledger (
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


--
-- Name: suppliers; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.suppliers (
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


--
-- Name: system_settings; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.system_settings (
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


--
-- Name: transfer_items; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.transfer_items (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    transfer_id uuid NOT NULL,
    product_id uuid NOT NULL,
    quantity numeric(10,3) NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    cost_price numeric(12,2) DEFAULT '0'::numeric NOT NULL,
    price numeric(12,2) DEFAULT '0'::numeric NOT NULL,
    store_id uuid
);


--
-- Name: transfers; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.transfers (
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


--
-- Name: user_stores; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.user_stores (
    user_id uuid NOT NULL,
    store_id uuid NOT NULL,
    role character varying(16) DEFAULT 'cashier'::character varying NOT NULL,
    permissions jsonb DEFAULT '{}'::jsonb NOT NULL,
    is_default boolean DEFAULT false NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL
);


--
-- Name: users; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.users (
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


--
-- Name: work_sessions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.work_sessions (
    id uuid NOT NULL,
    user_id uuid NOT NULL,
    login_time timestamp without time zone NOT NULL,
    logout_time timestamp without time zone,
    duration_hours numeric(5,2),
    created_at timestamp without time zone NOT NULL,
    store_id uuid
);


--
-- Name: write_off_items; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.write_off_items (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    write_off_id uuid NOT NULL,
    product_id uuid NOT NULL,
    quantity numeric(10,3) NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    cost_price numeric(12,2) DEFAULT '0'::numeric NOT NULL,
    price numeric(12,2) DEFAULT '0'::numeric NOT NULL,
    store_id uuid
);


--
-- Name: write_off_reasons; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.write_off_reasons (
    id uuid NOT NULL,
    name character varying(100) NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL
);


--
-- Name: write_offs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.write_offs (
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


--
-- Name: prro_settings id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.prro_settings ALTER COLUMN id SET DEFAULT nextval('public.prro_settings_id_seq'::regclass);


--
-- Name: barcodes barcodes_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.barcodes
    ADD CONSTRAINT barcodes_pkey PRIMARY KEY (id);


--
-- Name: cash_operations cash_operations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.cash_operations
    ADD CONSTRAINT cash_operations_pkey PRIMARY KEY (id);


--
-- Name: categories categories_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.categories
    ADD CONSTRAINT categories_pkey PRIMARY KEY (id);


--
-- Name: debtor_payments debtor_payments_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.debtor_payments
    ADD CONSTRAINT debtor_payments_pkey PRIMARY KEY (id);


--
-- Name: debtors debtors_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.debtors
    ADD CONSTRAINT debtors_pkey PRIMARY KEY (id);


--
-- Name: inventories inventories_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.inventories
    ADD CONSTRAINT inventories_pkey PRIMARY KEY (id);


--
-- Name: inventory_items inventory_items_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.inventory_items
    ADD CONSTRAINT inventory_items_pkey PRIMARY KEY (id);


--
-- Name: invoice_items invoice_items_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.invoice_items
    ADD CONSTRAINT invoice_items_pkey PRIMARY KEY (id);


--
-- Name: invoices invoices_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.invoices
    ADD CONSTRAINT invoices_pkey PRIMARY KEY (id);


--
-- Name: owners_db owners_db_db_name_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.owners_db
    ADD CONSTRAINT owners_db_db_name_key UNIQUE (db_name);


--
-- Name: owners_db owners_db_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.owners_db
    ADD CONSTRAINT owners_db_pkey PRIMARY KEY (owner_id);


--
-- Name: print_templates print_templates_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.print_templates
    ADD CONSTRAINT print_templates_pkey PRIMARY KEY (id);


--
-- Name: product_images product_images_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.product_images
    ADD CONSTRAINT product_images_pkey PRIMARY KEY (id);


--
-- Name: products products_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.products
    ADD CONSTRAINT products_pkey PRIMARY KEY (id);


--
-- Name: prro_queue_items prro_queue_items_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.prro_queue_items
    ADD CONSTRAINT prro_queue_items_pkey PRIMARY KEY (id);


--
-- Name: prro_settings prro_settings_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.prro_settings
    ADD CONSTRAINT prro_settings_pkey PRIMARY KEY (id);


--
-- Name: prro_shifts prro_shifts_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.prro_shifts
    ADD CONSTRAINT prro_shifts_pkey PRIMARY KEY (id);


--
-- Name: purchase_order_items purchase_order_items_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.purchase_order_items
    ADD CONSTRAINT purchase_order_items_pkey PRIMARY KEY (id);


--
-- Name: purchase_orders purchase_orders_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.purchase_orders
    ADD CONSTRAINT purchase_orders_pkey PRIMARY KEY (id);


--
-- Name: receipt_items receipt_items_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.receipt_items
    ADD CONSTRAINT receipt_items_pkey PRIMARY KEY (id);


--
-- Name: receipts receipts_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.receipts
    ADD CONSTRAINT receipts_pkey PRIMARY KEY (id);


--
-- Name: return_invoice_items return_invoice_items_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.return_invoice_items
    ADD CONSTRAINT return_invoice_items_pkey PRIMARY KEY (id);


--
-- Name: return_invoices return_invoices_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.return_invoices
    ADD CONSTRAINT return_invoices_pkey PRIMARY KEY (id);


--
-- Name: stock stock_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.stock
    ADD CONSTRAINT stock_pkey PRIMARY KEY (store_id, product_id);


--
-- Name: stores stores_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.stores
    ADD CONSTRAINT stores_pkey PRIMARY KEY (id);


--
-- Name: supplier_ledger supplier_ledger_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.supplier_ledger
    ADD CONSTRAINT supplier_ledger_pkey PRIMARY KEY (id);


--
-- Name: suppliers suppliers_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.suppliers
    ADD CONSTRAINT suppliers_pkey PRIMARY KEY (id);


--
-- Name: system_settings system_settings_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.system_settings
    ADD CONSTRAINT system_settings_pkey PRIMARY KEY (id);


--
-- Name: transfer_items transfer_items_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.transfer_items
    ADD CONSTRAINT transfer_items_pkey PRIMARY KEY (id);


--
-- Name: transfers transfers_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.transfers
    ADD CONSTRAINT transfers_pkey PRIMARY KEY (id);


--
-- Name: user_stores user_stores_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_stores
    ADD CONSTRAINT user_stores_pkey PRIMARY KEY (user_id, store_id);


--
-- Name: users users_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_pkey PRIMARY KEY (id);


--
-- Name: work_sessions work_sessions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.work_sessions
    ADD CONSTRAINT work_sessions_pkey PRIMARY KEY (id);


--
-- Name: write_off_items write_off_items_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.write_off_items
    ADD CONSTRAINT write_off_items_pkey PRIMARY KEY (id);


--
-- Name: write_off_reasons write_off_reasons_name_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.write_off_reasons
    ADD CONSTRAINT write_off_reasons_name_key UNIQUE (name);


--
-- Name: write_off_reasons write_off_reasons_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.write_off_reasons
    ADD CONSTRAINT write_off_reasons_pkey PRIMARY KEY (id);


--
-- Name: write_offs write_offs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.write_offs
    ADD CONSTRAINT write_offs_pkey PRIMARY KEY (id);


--
-- Name: ix_barcodes_barcode; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX ix_barcodes_barcode ON public.barcodes USING btree (barcode);


--
-- Name: ix_barcodes_product_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_barcodes_product_id ON public.barcodes USING btree (product_id);


--
-- Name: ix_barcodes_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_barcodes_store_created ON public.barcodes USING btree (store_id, created_at);


--
-- Name: ix_cash_operations_store_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_cash_operations_store_id ON public.cash_operations USING btree (store_id);


--
-- Name: ix_categories_name; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_categories_name ON public.categories USING btree (name);


--
-- Name: ix_categories_parent_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_categories_parent_id ON public.categories USING btree (parent_id);


--
-- Name: ix_categories_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_categories_store_created ON public.categories USING btree (store_id, created_at);


--
-- Name: ix_debtor_payments_debtor_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_debtor_payments_debtor_id ON public.debtor_payments USING btree (debtor_id);


--
-- Name: ix_debtor_payments_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_debtor_payments_store_created ON public.debtor_payments USING btree (store_id, created_at);


--
-- Name: ix_debtors_name; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_debtors_name ON public.debtors USING btree (name);


--
-- Name: ix_debtors_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_debtors_store_created ON public.debtors USING btree (store_id, created_at);


--
-- Name: ix_inventories_number; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_inventories_number ON public.inventories USING btree (number);


--
-- Name: ix_inventories_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_inventories_store_created ON public.inventories USING btree (store_id, created_at);


--
-- Name: ix_inventory_items_inventory_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_inventory_items_inventory_id ON public.inventory_items USING btree (inventory_id);


--
-- Name: ix_inventory_items_product_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_inventory_items_product_id ON public.inventory_items USING btree (product_id);


--
-- Name: ix_inventory_items_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_inventory_items_store_created ON public.inventory_items USING btree (store_id, created_at);


--
-- Name: ix_invoice_items_invoice_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_invoice_items_invoice_id ON public.invoice_items USING btree (invoice_id);


--
-- Name: ix_invoice_items_product_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_invoice_items_product_id ON public.invoice_items USING btree (product_id);


--
-- Name: ix_invoice_items_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_invoice_items_store_created ON public.invoice_items USING btree (store_id, created_at);


--
-- Name: ix_invoices_number; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_invoices_number ON public.invoices USING btree (number);


--
-- Name: ix_invoices_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_invoices_store_created ON public.invoices USING btree (store_id, created_at);


--
-- Name: ix_invoices_supplier_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_invoices_supplier_id ON public.invoices USING btree (supplier_id);


--
-- Name: ix_print_templates_type; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_print_templates_type ON public.print_templates USING btree (type);


--
-- Name: ix_product_images_product_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_product_images_product_id ON public.product_images USING btree (product_id);


--
-- Name: ix_product_images_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_product_images_store_created ON public.product_images USING btree (store_id, created_at);


--
-- Name: ix_products_barcode; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX ix_products_barcode ON public.products USING btree (barcode);


--
-- Name: ix_products_category_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_products_category_id ON public.products USING btree (category_id);


--
-- Name: ix_products_sku; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX ix_products_sku ON public.products USING btree (sku);


--
-- Name: ix_products_supplier_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_products_supplier_id ON public.products USING btree (supplier_id);


--
-- Name: ix_products_title; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_products_title ON public.products USING btree (title);


--
-- Name: ix_prro_queue_items_receipt_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_prro_queue_items_receipt_id ON public.prro_queue_items USING btree (receipt_id);


--
-- Name: ix_prro_queue_items_shift_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_prro_queue_items_shift_id ON public.prro_queue_items USING btree (shift_id);


--
-- Name: ix_prro_queue_items_status; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_prro_queue_items_status ON public.prro_queue_items USING btree (status);


--
-- Name: ix_prro_settings_key_name; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX ix_prro_settings_key_name ON public.prro_settings USING btree (key_name);


--
-- Name: ix_prro_shifts_shift_number; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_prro_shifts_shift_number ON public.prro_shifts USING btree (shift_number);


--
-- Name: ix_purchase_order_items_product_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_purchase_order_items_product_id ON public.purchase_order_items USING btree (product_id);


--
-- Name: ix_purchase_order_items_purchase_order_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_purchase_order_items_purchase_order_id ON public.purchase_order_items USING btree (purchase_order_id);


--
-- Name: ix_purchase_order_items_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_purchase_order_items_store_created ON public.purchase_order_items USING btree (store_id, created_at);


--
-- Name: ix_purchase_orders_invoice_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_purchase_orders_invoice_id ON public.purchase_orders USING btree (invoice_id);


--
-- Name: ix_purchase_orders_number; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_purchase_orders_number ON public.purchase_orders USING btree (number);


--
-- Name: ix_purchase_orders_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_purchase_orders_store_created ON public.purchase_orders USING btree (store_id, created_at);


--
-- Name: ix_purchase_orders_supplier_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_purchase_orders_supplier_id ON public.purchase_orders USING btree (supplier_id);


--
-- Name: ix_receipt_items_product_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_receipt_items_product_id ON public.receipt_items USING btree (product_id);


--
-- Name: ix_receipt_items_receipt_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_receipt_items_receipt_id ON public.receipt_items USING btree (receipt_id);


--
-- Name: ix_receipt_items_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_receipt_items_store_created ON public.receipt_items USING btree (store_id, created_at);


--
-- Name: ix_receipts_cashier_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_receipts_cashier_id ON public.receipts USING btree (cashier_id);


--
-- Name: ix_receipts_debtor_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_receipts_debtor_id ON public.receipts USING btree (debtor_id);


--
-- Name: ix_receipts_original_receipt_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_receipts_original_receipt_id ON public.receipts USING btree (original_receipt_id);


--
-- Name: ix_receipts_receipt_number; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_receipts_receipt_number ON public.receipts USING btree (receipt_number);


--
-- Name: ix_receipts_split_group_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_receipts_split_group_id ON public.receipts USING btree (split_group_id);


--
-- Name: ix_receipts_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_receipts_store_created ON public.receipts USING btree (store_id, created_at);


--
-- Name: ix_return_invoice_items_product_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_return_invoice_items_product_id ON public.return_invoice_items USING btree (product_id);


--
-- Name: ix_return_invoice_items_return_invoice_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_return_invoice_items_return_invoice_id ON public.return_invoice_items USING btree (return_invoice_id);


--
-- Name: ix_return_invoice_items_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_return_invoice_items_store_created ON public.return_invoice_items USING btree (store_id, created_at);


--
-- Name: ix_return_invoices_exchange_invoice_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_return_invoices_exchange_invoice_id ON public.return_invoices USING btree (exchange_invoice_id);


--
-- Name: ix_return_invoices_number; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_return_invoices_number ON public.return_invoices USING btree (number);


--
-- Name: ix_return_invoices_source_invoice_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_return_invoices_source_invoice_id ON public.return_invoices USING btree (source_invoice_id);


--
-- Name: ix_return_invoices_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_return_invoices_store_created ON public.return_invoices USING btree (store_id, created_at);


--
-- Name: ix_return_invoices_supplier_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_return_invoices_supplier_id ON public.return_invoices USING btree (supplier_id);


--
-- Name: ix_stock_product; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_stock_product ON public.stock USING btree (product_id);


--
-- Name: ix_stores_name; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_stores_name ON public.stores USING btree (name);


--
-- Name: ix_supplier_ledger_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_supplier_ledger_store_created ON public.supplier_ledger USING btree (store_id, created_at);


--
-- Name: ix_supplier_ledger_supplier_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_supplier_ledger_supplier_id ON public.supplier_ledger USING btree (supplier_id);


--
-- Name: ix_suppliers_name; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_suppliers_name ON public.suppliers USING btree (name);


--
-- Name: ix_system_settings_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX ix_system_settings_key ON public.system_settings USING btree (store_id, key);


--
-- Name: ix_system_settings_module; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_system_settings_module ON public.system_settings USING btree (module);


--
-- Name: ix_system_settings_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_system_settings_store_created ON public.system_settings USING btree (store_id, created_at);


--
-- Name: ix_transfer_items_product_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_transfer_items_product_id ON public.transfer_items USING btree (product_id);


--
-- Name: ix_transfer_items_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_transfer_items_store_created ON public.transfer_items USING btree (store_id, created_at);


--
-- Name: ix_transfer_items_transfer_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_transfer_items_transfer_id ON public.transfer_items USING btree (transfer_id);


--
-- Name: ix_transfers_number; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_transfers_number ON public.transfers USING btree (number);


--
-- Name: ix_transfers_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_transfers_store_created ON public.transfers USING btree (store_id, created_at);


--
-- Name: ix_user_stores_role; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_user_stores_role ON public.user_stores USING btree (role);


--
-- Name: ix_user_stores_store; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_user_stores_store ON public.user_stores USING btree (store_id);


--
-- Name: ix_users_login; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX ix_users_login ON public.users USING btree (login);


--
-- Name: ix_work_sessions_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_work_sessions_store_created ON public.work_sessions USING btree (store_id, created_at);


--
-- Name: ix_work_sessions_user_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_work_sessions_user_id ON public.work_sessions USING btree (user_id);


--
-- Name: ix_write_off_items_product_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_write_off_items_product_id ON public.write_off_items USING btree (product_id);


--
-- Name: ix_write_off_items_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_write_off_items_store_created ON public.write_off_items USING btree (store_id, created_at);


--
-- Name: ix_write_off_items_write_off_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_write_off_items_write_off_id ON public.write_off_items USING btree (write_off_id);


--
-- Name: ix_write_offs_number; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_write_offs_number ON public.write_offs USING btree (number);


--
-- Name: ix_write_offs_store_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_write_offs_store_created ON public.write_offs USING btree (store_id, created_at);


--
-- Name: uq_print_templates_default_per_type; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX uq_print_templates_default_per_type ON public.print_templates USING btree (type, store_id) WHERE (is_default = true);


--
-- Name: barcodes barcodes_product_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.barcodes
    ADD CONSTRAINT barcodes_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE CASCADE;


--
-- Name: categories categories_parent_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.categories
    ADD CONSTRAINT categories_parent_id_fkey FOREIGN KEY (parent_id) REFERENCES public.categories(id) ON DELETE SET NULL;


--
-- Name: debtor_payments debtor_payments_debtor_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.debtor_payments
    ADD CONSTRAINT debtor_payments_debtor_id_fkey FOREIGN KEY (debtor_id) REFERENCES public.debtors(id) ON DELETE CASCADE;


--
-- Name: barcodes fk_barcodes_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.barcodes
    ADD CONSTRAINT fk_barcodes_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: categories fk_categories_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.categories
    ADD CONSTRAINT fk_categories_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: debtor_payments fk_debtor_payments_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.debtor_payments
    ADD CONSTRAINT fk_debtor_payments_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: debtors fk_debtors_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.debtors
    ADD CONSTRAINT fk_debtors_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: inventories fk_inventories_created_by_id; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.inventories
    ADD CONSTRAINT fk_inventories_created_by_id FOREIGN KEY (created_by_id) REFERENCES public.users(id) ON DELETE RESTRICT;


--
-- Name: inventories fk_inventories_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.inventories
    ADD CONSTRAINT fk_inventories_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: inventory_items fk_inventory_items_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.inventory_items
    ADD CONSTRAINT fk_inventory_items_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: invoice_items fk_invoice_items_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.invoice_items
    ADD CONSTRAINT fk_invoice_items_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: invoices fk_invoices_created_by_id; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.invoices
    ADD CONSTRAINT fk_invoices_created_by_id FOREIGN KEY (created_by_id) REFERENCES public.users(id) ON DELETE RESTRICT;


--
-- Name: invoices fk_invoices_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.invoices
    ADD CONSTRAINT fk_invoices_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: print_templates fk_print_templates_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.print_templates
    ADD CONSTRAINT fk_print_templates_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: product_images fk_product_images_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.product_images
    ADD CONSTRAINT fk_product_images_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: purchase_order_items fk_purchase_order_items_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.purchase_order_items
    ADD CONSTRAINT fk_purchase_order_items_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: purchase_orders fk_purchase_orders_created_by_id; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.purchase_orders
    ADD CONSTRAINT fk_purchase_orders_created_by_id FOREIGN KEY (created_by_id) REFERENCES public.users(id) ON DELETE RESTRICT;


--
-- Name: purchase_orders fk_purchase_orders_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.purchase_orders
    ADD CONSTRAINT fk_purchase_orders_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: receipt_items fk_receipt_items_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.receipt_items
    ADD CONSTRAINT fk_receipt_items_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: receipts fk_receipts_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.receipts
    ADD CONSTRAINT fk_receipts_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: return_invoice_items fk_return_invoice_items_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.return_invoice_items
    ADD CONSTRAINT fk_return_invoice_items_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: return_invoices fk_return_invoices_created_by_id; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.return_invoices
    ADD CONSTRAINT fk_return_invoices_created_by_id FOREIGN KEY (created_by_id) REFERENCES public.users(id) ON DELETE RESTRICT;


--
-- Name: return_invoices fk_return_invoices_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.return_invoices
    ADD CONSTRAINT fk_return_invoices_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: supplier_ledger fk_supplier_ledger_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.supplier_ledger
    ADD CONSTRAINT fk_supplier_ledger_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: system_settings fk_system_settings_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.system_settings
    ADD CONSTRAINT fk_system_settings_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: transfer_items fk_transfer_items_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.transfer_items
    ADD CONSTRAINT fk_transfer_items_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: transfers fk_transfers_created_by_id; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.transfers
    ADD CONSTRAINT fk_transfers_created_by_id FOREIGN KEY (created_by_id) REFERENCES public.users(id) ON DELETE RESTRICT;


--
-- Name: transfers fk_transfers_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.transfers
    ADD CONSTRAINT fk_transfers_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: work_sessions fk_work_sessions_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.work_sessions
    ADD CONSTRAINT fk_work_sessions_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: write_off_items fk_write_off_items_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.write_off_items
    ADD CONSTRAINT fk_write_off_items_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: write_offs fk_write_offs_created_by_id; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.write_offs
    ADD CONSTRAINT fk_write_offs_created_by_id FOREIGN KEY (created_by_id) REFERENCES public.users(id) ON DELETE RESTRICT;


--
-- Name: write_offs fk_write_offs_store; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.write_offs
    ADD CONSTRAINT fk_write_offs_store FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: inventory_items inventory_items_inventory_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.inventory_items
    ADD CONSTRAINT inventory_items_inventory_id_fkey FOREIGN KEY (inventory_id) REFERENCES public.inventories(id) ON DELETE CASCADE;


--
-- Name: inventory_items inventory_items_product_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.inventory_items
    ADD CONSTRAINT inventory_items_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE RESTRICT;


--
-- Name: invoice_items invoice_items_invoice_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.invoice_items
    ADD CONSTRAINT invoice_items_invoice_id_fkey FOREIGN KEY (invoice_id) REFERENCES public.invoices(id) ON DELETE CASCADE;


--
-- Name: invoice_items invoice_items_product_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.invoice_items
    ADD CONSTRAINT invoice_items_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE RESTRICT;


--
-- Name: invoices invoices_supplier_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.invoices
    ADD CONSTRAINT invoices_supplier_id_fkey FOREIGN KEY (supplier_id) REFERENCES public.suppliers(id) ON DELETE RESTRICT;


--
-- Name: owners_db owners_db_owner_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.owners_db
    ADD CONSTRAINT owners_db_owner_id_fkey FOREIGN KEY (owner_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: product_images product_images_product_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.product_images
    ADD CONSTRAINT product_images_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE CASCADE;


--
-- Name: products products_category_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.products
    ADD CONSTRAINT products_category_id_fkey FOREIGN KEY (category_id) REFERENCES public.categories(id) ON DELETE SET NULL;


--
-- Name: products products_supplier_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.products
    ADD CONSTRAINT products_supplier_id_fkey FOREIGN KEY (supplier_id) REFERENCES public.suppliers(id) ON DELETE SET NULL;


--
-- Name: prro_queue_items prro_queue_items_receipt_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.prro_queue_items
    ADD CONSTRAINT prro_queue_items_receipt_id_fkey FOREIGN KEY (receipt_id) REFERENCES public.receipts(id) ON DELETE SET NULL;


--
-- Name: prro_queue_items prro_queue_items_shift_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.prro_queue_items
    ADD CONSTRAINT prro_queue_items_shift_id_fkey FOREIGN KEY (shift_id) REFERENCES public.prro_shifts(id) ON DELETE SET NULL;


--
-- Name: purchase_order_items purchase_order_items_product_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.purchase_order_items
    ADD CONSTRAINT purchase_order_items_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE RESTRICT;


--
-- Name: purchase_order_items purchase_order_items_purchase_order_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.purchase_order_items
    ADD CONSTRAINT purchase_order_items_purchase_order_id_fkey FOREIGN KEY (purchase_order_id) REFERENCES public.purchase_orders(id) ON DELETE CASCADE;


--
-- Name: purchase_orders purchase_orders_invoice_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.purchase_orders
    ADD CONSTRAINT purchase_orders_invoice_id_fkey FOREIGN KEY (invoice_id) REFERENCES public.invoices(id) ON DELETE SET NULL;


--
-- Name: purchase_orders purchase_orders_supplier_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.purchase_orders
    ADD CONSTRAINT purchase_orders_supplier_id_fkey FOREIGN KEY (supplier_id) REFERENCES public.suppliers(id) ON DELETE RESTRICT;


--
-- Name: receipt_items receipt_items_product_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.receipt_items
    ADD CONSTRAINT receipt_items_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE RESTRICT;


--
-- Name: receipt_items receipt_items_receipt_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.receipt_items
    ADD CONSTRAINT receipt_items_receipt_id_fkey FOREIGN KEY (receipt_id) REFERENCES public.receipts(id) ON DELETE CASCADE;


--
-- Name: receipts receipts_cashier_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.receipts
    ADD CONSTRAINT receipts_cashier_id_fkey FOREIGN KEY (cashier_id) REFERENCES public.users(id) ON DELETE RESTRICT;


--
-- Name: receipts receipts_debtor_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.receipts
    ADD CONSTRAINT receipts_debtor_id_fkey FOREIGN KEY (debtor_id) REFERENCES public.debtors(id) ON DELETE SET NULL;


--
-- Name: receipts receipts_original_receipt_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.receipts
    ADD CONSTRAINT receipts_original_receipt_id_fkey FOREIGN KEY (original_receipt_id) REFERENCES public.receipts(id) ON DELETE SET NULL;


--
-- Name: receipts receipts_split_group_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.receipts
    ADD CONSTRAINT receipts_split_group_id_fkey FOREIGN KEY (split_group_id) REFERENCES public.receipts(id) ON DELETE SET NULL;


--
-- Name: return_invoice_items return_invoice_items_product_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.return_invoice_items
    ADD CONSTRAINT return_invoice_items_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE RESTRICT;


--
-- Name: return_invoice_items return_invoice_items_return_invoice_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.return_invoice_items
    ADD CONSTRAINT return_invoice_items_return_invoice_id_fkey FOREIGN KEY (return_invoice_id) REFERENCES public.return_invoices(id) ON DELETE CASCADE;


--
-- Name: return_invoices return_invoices_exchange_invoice_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.return_invoices
    ADD CONSTRAINT return_invoices_exchange_invoice_id_fkey FOREIGN KEY (exchange_invoice_id) REFERENCES public.invoices(id) ON DELETE SET NULL;


--
-- Name: return_invoices return_invoices_source_invoice_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.return_invoices
    ADD CONSTRAINT return_invoices_source_invoice_id_fkey FOREIGN KEY (source_invoice_id) REFERENCES public.invoices(id) ON DELETE SET NULL;


--
-- Name: return_invoices return_invoices_supplier_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.return_invoices
    ADD CONSTRAINT return_invoices_supplier_id_fkey FOREIGN KEY (supplier_id) REFERENCES public.suppliers(id) ON DELETE RESTRICT;


--
-- Name: stock stock_product_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.stock
    ADD CONSTRAINT stock_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE CASCADE;


--
-- Name: stock stock_store_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.stock
    ADD CONSTRAINT stock_store_id_fkey FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: supplier_ledger supplier_ledger_supplier_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.supplier_ledger
    ADD CONSTRAINT supplier_ledger_supplier_id_fkey FOREIGN KEY (supplier_id) REFERENCES public.suppliers(id) ON DELETE RESTRICT;


--
-- Name: transfer_items transfer_items_product_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.transfer_items
    ADD CONSTRAINT transfer_items_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE RESTRICT;


--
-- Name: transfer_items transfer_items_transfer_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.transfer_items
    ADD CONSTRAINT transfer_items_transfer_id_fkey FOREIGN KEY (transfer_id) REFERENCES public.transfers(id) ON DELETE CASCADE;


--
-- Name: user_stores user_stores_store_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_stores
    ADD CONSTRAINT user_stores_store_id_fkey FOREIGN KEY (store_id) REFERENCES public.stores(id) ON DELETE CASCADE;


--
-- Name: user_stores user_stores_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_stores
    ADD CONSTRAINT user_stores_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: work_sessions work_sessions_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.work_sessions
    ADD CONSTRAINT work_sessions_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: write_off_items write_off_items_product_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.write_off_items
    ADD CONSTRAINT write_off_items_product_id_fkey FOREIGN KEY (product_id) REFERENCES public.products(id) ON DELETE RESTRICT;


--
-- Name: write_off_items write_off_items_write_off_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.write_off_items
    ADD CONSTRAINT write_off_items_write_off_id_fkey FOREIGN KEY (write_off_id) REFERENCES public.write_offs(id) ON DELETE CASCADE;


--
-- Name: barcodes; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.barcodes ENABLE ROW LEVEL SECURITY;

--
-- Name: barcodes barcodes_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY barcodes_store_isolation ON public.barcodes USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: categories; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.categories ENABLE ROW LEVEL SECURITY;

--
-- Name: categories categories_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY categories_store_isolation ON public.categories USING (((store_id IS NULL) OR (store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id IS NULL) OR (store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: debtor_payments; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.debtor_payments ENABLE ROW LEVEL SECURITY;

--
-- Name: debtor_payments debtor_payments_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY debtor_payments_store_isolation ON public.debtor_payments USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: debtors; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.debtors ENABLE ROW LEVEL SECURITY;

--
-- Name: debtors debtors_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY debtors_store_isolation ON public.debtors USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: inventories; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.inventories ENABLE ROW LEVEL SECURITY;

--
-- Name: inventories inventories_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY inventories_store_isolation ON public.inventories USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: inventory_items; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.inventory_items ENABLE ROW LEVEL SECURITY;

--
-- Name: inventory_items inventory_items_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY inventory_items_store_isolation ON public.inventory_items USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: invoice_items; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.invoice_items ENABLE ROW LEVEL SECURITY;

--
-- Name: invoice_items invoice_items_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY invoice_items_store_isolation ON public.invoice_items USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: invoices; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.invoices ENABLE ROW LEVEL SECURITY;

--
-- Name: invoices invoices_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY invoices_store_isolation ON public.invoices USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: product_images; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.product_images ENABLE ROW LEVEL SECURITY;

--
-- Name: product_images product_images_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY product_images_store_isolation ON public.product_images USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: purchase_order_items; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.purchase_order_items ENABLE ROW LEVEL SECURITY;

--
-- Name: purchase_order_items purchase_order_items_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY purchase_order_items_store_isolation ON public.purchase_order_items USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: purchase_orders; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.purchase_orders ENABLE ROW LEVEL SECURITY;

--
-- Name: purchase_orders purchase_orders_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY purchase_orders_store_isolation ON public.purchase_orders USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: receipt_items; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.receipt_items ENABLE ROW LEVEL SECURITY;

--
-- Name: receipt_items receipt_items_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY receipt_items_store_isolation ON public.receipt_items USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: receipts; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.receipts ENABLE ROW LEVEL SECURITY;

--
-- Name: receipts receipts_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY receipts_store_isolation ON public.receipts USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: return_invoice_items; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.return_invoice_items ENABLE ROW LEVEL SECURITY;

--
-- Name: return_invoice_items return_invoice_items_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY return_invoice_items_store_isolation ON public.return_invoice_items USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: return_invoices; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.return_invoices ENABLE ROW LEVEL SECURITY;

--
-- Name: return_invoices return_invoices_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY return_invoices_store_isolation ON public.return_invoices USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: stock; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.stock ENABLE ROW LEVEL SECURITY;

--
-- Name: stock stock_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY stock_store_isolation ON public.stock USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: stores; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.stores ENABLE ROW LEVEL SECURITY;

--
-- Name: stores stores_access; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY stores_access ON public.stores USING ((id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))));


--
-- Name: supplier_ledger; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.supplier_ledger ENABLE ROW LEVEL SECURITY;

--
-- Name: supplier_ledger supplier_ledger_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY supplier_ledger_store_isolation ON public.supplier_ledger USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: system_settings; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.system_settings ENABLE ROW LEVEL SECURITY;

--
-- Name: system_settings system_settings_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY system_settings_store_isolation ON public.system_settings USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: transfer_items; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.transfer_items ENABLE ROW LEVEL SECURITY;

--
-- Name: transfer_items transfer_items_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY transfer_items_store_isolation ON public.transfer_items USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: transfers; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.transfers ENABLE ROW LEVEL SECURITY;

--
-- Name: transfers transfers_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY transfers_store_isolation ON public.transfers USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: user_stores; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.user_stores ENABLE ROW LEVEL SECURITY;

--
-- Name: user_stores user_stores_self; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY user_stores_self ON public.user_stores USING ((user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid));


--
-- Name: work_sessions; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.work_sessions ENABLE ROW LEVEL SECURITY;

--
-- Name: work_sessions work_sessions_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY work_sessions_store_isolation ON public.work_sessions USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: write_off_items; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.write_off_items ENABLE ROW LEVEL SECURITY;

--
-- Name: write_off_items write_off_items_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY write_off_items_store_isolation ON public.write_off_items USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- Name: write_offs; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.write_offs ENABLE ROW LEVEL SECURITY;

--
-- Name: write_offs write_offs_store_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY write_offs_store_isolation ON public.write_offs USING (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid))))) WITH CHECK (((store_id = (NULLIF(current_setting('app.store_id'::text, true), ''::text))::uuid) OR (store_id IN ( SELECT user_stores.store_id
   FROM public.user_stores
  WHERE (user_stores.user_id = (NULLIF(current_setting('app.user_id'::text, true), ''::text))::uuid)))));


--
-- PostgreSQL database dump complete
--



SET search_path = public;

-- ============================================================================
-- Тестовий seed для Rust integration-тестів
-- (див. crates/torgashka-infrastructure/tests/*.rs, crates/torgashka-api/tests/*.rs)
-- Фіксовані UUID використовуються тестами як контекст запиту.
-- ============================================================================
INSERT INTO stores (id, name) VALUES
  ('65d5db51-672f-4a38-9c1e-f36c5feb5374', 'Білий магазин'),
  ('5e840d11-6b9b-4f6f-a6e4-000d1bb0a307', 'Жовтий магазин'),
  ('d9be9608-c011-49be-b776-3317ca5e9af6', 'Тест Магазин C')
ON CONFLICT (id) DO NOTHING;

INSERT INTO users (id, name, login, password_hash, role, is_active, created_at, updated_at, onboarding_completed)
VALUES (
  'e30d480c-ef3b-4d0e-8808-0c745196d3d8', 'ФОП Мельничук', 'igor2104@i.ua',
  '$2b$12$4XDCv4sfOnJem6tUbNppD.8gh8Uc6Y.8Teci3LHweA/qQOLpSFm9e',
  'owner'::public.user_role, true, now(), now(), true
)
ON CONFLICT (id) DO NOTHING;

INSERT INTO user_stores (user_id, store_id, role, permissions, is_default, created_at)
SELECT 'e30d480c-ef3b-4d0e-8808-0c745196d3d8', s.id, 'owner', '{}'::jsonb, true, now()
FROM stores s
WHERE s.id IN (
  '65d5db51-672f-4a38-9c1e-f36c5feb5374',
  '5e840d11-6b9b-4f6f-a6e4-000d1bb0a307',
  'd9be9608-c011-49be-b776-3317ca5e9af6'
);

-- ============================================================================
-- Seed: налаштування та шаблон для точки-донора (store_settings_isolation)
-- Тест вимагає: src_settings > 0 і src_templates > 0 для 65d5db51.
-- ============================================================================
INSERT INTO system_settings (id, module, key, value, value_type, label, description, options, is_active, created_at, updated_at, store_id) VALUES
  ('a0000000-0000-4000-8000-000000000001', 'general', 'company_name', 'ФОП Мельничук', 'string', 'Назва магазину', 'Виводиться в чеках', NULL, true, now(), now(), '65d5db51-672f-4a38-9c1e-f36c5feb5374'),
  ('a0000000-0000-4000-8000-000000000002', 'pos', 'allow_negative_stock', 'false', 'boolean', 'Торгівля в мінус', 'Дозволити продаж в мінус', NULL, true, now(), now(), '65d5db51-672f-4a38-9c1e-f36c5feb5374'),
  ('a0000000-0000-4000-8000-000000000003', 'printing', 'default_template', 'receipt_80mm', 'string', 'Шаблон за замовчуванням', 'Для чеків', NULL, true, now(), now(), '65d5db51-672f-4a38-9c1e-f36c5feb5374');

INSERT INTO print_templates (id, name, type, content, variables, is_default, is_active, created_at, updated_at, store_id) VALUES
  ('b0000000-0000-4000-8000-000000000001', 'receipt_80mm (test)', 'receipt', '<html><body>{{items}}</body></html>', '[{"key": "shop_name", "label": "Магазин", "default": "Мій"}]', false, true, now(), now(), '65d5db51-672f-4a38-9c1e-f36c5feb5374');
